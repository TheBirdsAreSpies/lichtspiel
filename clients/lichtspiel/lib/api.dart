import 'dart:async';
import 'dart:convert';
import 'dart:io';
import 'package:flutter/foundation.dart';
import 'package:flutter_secure_storage/flutter_secure_storage.dart';
import 'package:http/http.dart' as http;
import 'package:http/io_client.dart';
import 'package:shared_preferences/shared_preferences.dart';

class ApiError implements Exception {
  final int status;
  final String message;
  final bool needsCode;
  final Object? cause; // for status 0: the network error behind it
  ApiError(this.status, this.message, {this.needsCode = false, this.cause});
  @override
  String toString() => message;
}

/// Talks to a Lichtspiel server: token login, JSON requests, stream/picture URLs, progress reports.
class Api {
  String base; // e.g. https://nas:8484
  String? token;
  Api(this.base, [this.token]);

  /// Called when the server no longer accepts the token (signed out elsewhere, device removed, backup restored).
  static void Function(Api)? onUnauthorized;

  /// The token lives in the system's keychain (Keychain on Apple devices, Keystore on Android,
  /// Credential Manager on Windows); the server address stays in the plain settings.
  /// On the Mac the classic login keychain is used: the newer one needs the Keychain Sharing
  /// entitlement and a provisioning profile, so a self-signed build would only run on the Mac that built it.
  static const _secure = FlutterSecureStorage(mOptions: MacOsOptions(usesDataProtectionKeychain: false));

  static Future<Api?> restore() async {
    final p = await SharedPreferences.getInstance();
    final b = p.getString('server');
    final t = await _readToken(p);
    if (b == null || t == null) return null;
    return Api(b, t);
  }

  /// Older versions kept the token in the plain settings: move it to the keychain once.
  /// If the keychain can't be used (e.g. an unsigned Mac build), the plain setting keeps working.
  /// A token in the plain settings is always the newer one (save() only writes it there when the keychain refused).
  static Future<String?> _readToken(SharedPreferences p) async {
    String? t;
    try {
      t = await _secure.read(key: 'token');
    } catch (_) {}
    final old = p.getString('token');
    if (old == null) return t;
    if (t != old) {
      try {
        await _secure.write(key: 'token', value: old);
        t = old;
      } catch (_) {
        return old;
      }
    }
    await p.remove('token');
    return t;
  }

  Future<void> save() async {
    final p = await SharedPreferences.getInstance();
    await p.setString('server', base);
    if (token == null) return;
    try {
      await _secure.write(key: 'token', value: token!);
      await p.remove('token');
    } catch (_) {
      await p.setString('token', token!);
    }
  }

  Future<void> forget() async {
    final p = await SharedPreferences.getInstance();
    try {
      await _secure.delete(key: 'token');
    } catch (_) {}
    await p.remove('token');
    token = null;
  }

  /// "nas" -> "http://nas:8484"; keeps an explicit scheme and port ("[fd00::5]" for IPv6).
  static String normalize(String s) {
    s = s.trim().replaceAll(RegExp(r'/+$'), '');
    if (!s.startsWith('http://') && !s.startsWith('https://')) s = 'http://$s';
    final u = Uri.tryParse(s);
    if (u != null && u.host.isNotEmpty && !u.hasPort) s = u.replace(port: 8484).toString();
    return s;
  }

  /// How long connecting (including the TLS handshake) and a normal request may take.
  static Duration timeout = const Duration(seconds: 10);

  /// Changes can make the server work for a while (online matching, backups, plugins, uploaded pictures).
  static const slowTimeout = Duration(minutes: 2);

  /// What went wrong on the way to the server, in words for the user. [https]: the address used https.
  static String describe(Object e, {bool https = false}) {
    final s = '$e';
    if (e is HandshakeException && RegExp('CERTIFICATE|certificate').hasMatch(s)) return 'Das Zertifikat des Servers ist ungültig oder nicht vertrauenswürdig';
    final timedOut = e is TimeoutException || e is http.RequestAbortedException || s.contains('timed out');
    if (https && (timedOut || e is HandshakeException || e is TlsException)) return 'Server spricht kein HTTPS – http:// verwenden';
    if (timedOut) return 'Server antwortet nicht (Zeitüberschreitung)';
    if (s.contains('Failed host lookup')) return 'Adresse nicht gefunden';
    final code = e is SocketException ? e.osError?.errorCode : null;
    if (s.contains('Connection refused') || const {61, 111, 10061}.contains(code)) return 'Verbindung abgelehnt – läuft der Server?';
    return 'Server nicht erreichbar';
  }

  /// The server's known error texts in German; others are shown as they come.
  static String german(String msg) {
    const known = {
      'User name or password is wrong': 'Benutzername oder Passwort falsch',
      'Wrong PIN': 'Falsche PIN',
      'A PIN is 4 to 8 digits': 'Eine PIN hat 4 bis 8 Ziffern',
      'Too many wrong PINs; try again in a few minutes': 'Zu viele falsche PINs – versuch es in ein paar Minuten noch einmal',
      'Enter the code from your authenticator app': 'Gib den Code aus deiner Authenticator-App ein',
      'That code is wrong': 'Dieser Code ist falsch',
      'That code was already used; wait for the next one': 'Dieser Code wurde schon verwendet; warte auf den nächsten',
      'The password is wrong': 'Das Passwort ist falsch',
      'The old password is wrong': 'Das alte Passwort ist falsch',
      'No such profile': 'Dieses Profil gibt es nicht',
      'Profiles on the sign-in screen are switched off': 'Profile auf dem Anmeldebildschirm sind ausgeschaltet',
      'Profiles can only be picked in the local network; sign in with your password': 'Profile lassen sich nur im lokalen Netz auswählen – melde dich mit deinem Passwort an',
      'Only an admin can do this': 'Das darf nur ein Admin',
      "You don't have access to this library": 'Du hast keinen Zugriff auf diese Bibliothek',
    };
    final locked = RegExp(r'^Too many wrong attempts; try again in (\d+) minute\(s\)$').firstMatch(msg);
    if (locked != null) return 'Zu viele falsche Versuche – versuch es in ${locked[1]} Minute(n) noch einmal';
    return known[msg] ?? msg;
  }

  /// Checks that [address] answers. An https address whose server only speaks plain http
  /// is tried once with http (as the network search does); http is never switched to https.
  static Future<Api> reach(String address) async {
    final api = Api(normalize(address));
    try {
      await api.status();
      return api;
    } on ApiError catch (e) {
      if (e.status != 0 || e.message != 'Server spricht kein HTTPS – http:// verwenden') rethrow;
      final plain = Api(api.base.replaceFirst('https://', 'http://'));
      try {
        await plain.status();
      } catch (_) {
        throw e;
      }
      return plain;
    }
  }

  /// A client whose connecting gives up after [timeout]; a test's client (http.runWithClient) is used as it is.
  /// shortcut: a TLS handshake that never finishes is given up but its socket stays open (dart:io only cancels the
  /// TCP connect); harmless while it is one socket per failed try, revisit if dart:io gains a handshake timeout.
  static http.Client _client() {
    final c = http.Client();
    if (c is! IOClient) return c;
    c.close();
    return IOClient(HttpClient()..connectionTimeout = timeout);
  }

  /// Sends one request; when it takes too long it is aborted, which closes its connection.
  /// Network failures become an [ApiError] with status 0 and a German message.
  Future<dynamic> _send(String method, String path, Object? body) async {
    final url = Uri.parse('$base$path');
    final stop = Completer<void>();
    final timer = Timer(method == 'GET' ? timeout : slowTimeout, stop.complete);
    final client = _client();
    final req = http.AbortableRequest(method, url, abortTrigger: stop.future)..headers.addAll(_headers);
    if (body != null) req.body = jsonEncode(body);
    final http.Response r;
    try {
      r = await http.Response.fromStream(await client.send(req));
    } catch (e) {
      throw ApiError(0, describe(e, https: url.isScheme('https')), cause: e);
    } finally {
      timer.cancel();
      client.close();
    }
    return _decode(r);
  }

  Map<String, String> get _headers => {
        'Content-Type': 'application/json',
        if (token != null) 'X-Token': token!,
      };

  dynamic _decode(http.Response r) {
    dynamic body;
    try {
      body = r.body.isEmpty ? null : jsonDecode(utf8.decode(r.bodyBytes));
    } catch (_) {}
    if (r.statusCode >= 400) {
      if (r.statusCode == 401 && token != null) onUnauthorized?.call(this);
      final msg = body is Map && body['error'] != null ? german('${body['error']}') : 'Fehler ${r.statusCode}';
      throw ApiError(r.statusCode, msg, needsCode: body is Map && body['needs_code'] == true);
    }
    return body;
  }

  Future<dynamic> get(String path) => _send('GET', path, null);

  Future<dynamic> post(String path, [Object? body]) => _send('POST', path, body);

  Future<dynamic> put(String path, [Object? body]) => _send('PUT', path, body);

  /// Any other method (PATCH, ...) with a JSON body.
  Future<dynamic> request(String method, String path, [Object? body]) => _send(method, path, body);

  Future<Map<String, dynamic>> status() async => Map<String, dynamic>.from(await get('/api/status'));

  /// Signs in and keeps the token. Throws [ApiError] with needsCode when two-step sign-in wants a code.
  Future<void> login(String user, String pass, {String? code}) async {
    final r = await post('/api/login', {
      'username': user,
      'password': pass,
      'device': 'Lichtspiel App',
      if (code != null) 'code': code,
    });
    token = r['token'];
    await save();
  }

  List<Map<String, dynamic>> metadata(dynamic r) =>
      ((r?['MediaContainer']?['Metadata'] as List?) ?? const []).map((e) => Map<String, dynamic>.from(e)).toList();

  Future<List<Map<String, dynamic>>> sections() async =>
      ((await get('/library/sections'))['MediaContainer']?['Directory'] as List? ?? const [])
          .map((e) => Map<String, dynamic>.from(e))
          .toList();

  Future<List<Map<String, dynamic>>> hubs() async =>
      ((await get('/hubs'))['MediaContainer']?['Hub'] as List? ?? const []).map((e) => Map<String, dynamic>.from(e)).toList();

  Future<List<Map<String, dynamic>>> items(String sectionId, {String sort = 'title', int offset = 0, int limit = 60}) async =>
      metadata(await get('/library/sections/$sectionId/all?sort=$sort&offset=$offset&limit=$limit'));

  static String query(Map<String, String> q) => q.entries.map((e) => '${Uri.encodeQueryComponent(e.key)}=${Uri.encodeQueryComponent(e.value)}').join('&');

  /// One page of a library with the web app's filters and sorting: [q] holds type, sort, year>, year<, q,
  /// container and the filters by name (genre=Drama, unwatched=true, ...). total is the size without paging.
  Future<({List<Map<String, dynamic>> items, int total})> list(String sectionId, Map<String, String> q, {int start = 0, int size = 60}) async {
    final r = await get('/library/sections/$sectionId/all?${query({...q, 'start': '$start', 'size': '$size'})}');
    final items = metadata(r);
    return (items: items, total: (r?['MediaContainer']?['totalSize'] ?? items.length) as int);
  }

  /// The first letters of the titles with how many each has (for the letter bar).
  Future<List<({String key, int size})>> firstCharacter(String sectionId, Map<String, String> q) async {
    final r = await get('/library/sections/$sectionId/firstCharacter?${query({...q}..remove('sort'))}');
    return ((r?['MediaContainer']?['Directory'] as List?) ?? const []).map((e) => (key: '${e['key']}', size: (e['size'] ?? 0) as int)).toList();
  }

  /// The kinds of filter of a library with their values and counts (genre, year, age rating, unwatched, ...).
  Future<List<Map<String, dynamic>>> filters(String sectionId, {String type = ''}) async =>
      ((await get('/library/sections/$sectionId/filters${type.isEmpty ? '' : '?type=$type'}'))['MediaContainer']?['Filter'] as List? ?? const [])
          .map((e) => Map<String, dynamic>.from(e))
          .toList();

  /// An actor and everything they are in: name, photo, size and Metadata (with the role).
  Future<Map<String, dynamic>> person(String key, String name) async {
    final r = await get('/library/people/${Uri.encodeComponent(key)}?name=${Uri.encodeQueryComponent(name)}');
    return Map<String, dynamic>.from(r?['MediaContainer'] ?? r);
  }

  /// From TMDB: biography, birth, more photos and the filmography (each with `item` when it is in a library).
  Future<Map<String, dynamic>> personDetails(String key, String name, {bool refresh = false}) async {
    final r = await get('/library/people/${Uri.encodeComponent(key)}/details?name=${Uri.encodeQueryComponent(name)}${refresh ? '&refresh=1' : ''}');
    return Map<String, dynamic>.from(r?['MediaContainer'] ?? r);
  }

  Future<Map<String, dynamic>> item(String id) async => metadata(await get('/library/metadata/$id')).first;
  Future<List<Map<String, dynamic>>> children(String id) async => metadata(await get('/library/metadata/$id/children'));
  Future<List<Map<String, dynamic>>> allLeaves(String id) async => metadata(await get('/library/metadata/$id/allLeaves'));

  Future<List<Map<String, dynamic>>> chapters(String id) async {
    try {
      return ((await get('/library/metadata/$id/chapters'))['MediaContainer']?['Chapter'] as List? ?? const [])
          .map((e) => Map<String, dynamic>.from(e))
          .toList();
    } catch (e) {
      debugPrint('Loading chapters from $base failed: $e');
      return const [];
    }
  }

  /// "Other editions", "More by the director", ... as hubs with a title and Metadata.
  Future<List<Map<String, dynamic>>> related(String id, {int count = 20}) async {
    try {
      return ((await get('/library/metadata/$id/related?count=$count'))['MediaContainer']?['Hub'] as List? ?? const [])
          .map((e) => Map<String, dynamic>.from(e))
          .toList();
    } catch (e) {
      debugPrint('Loading related items from $base failed: $e');
      return const [];
    }
  }

  Future<void> setWatchedAll(List<String> ids, bool watched) async => post('/:/${watched ? 'scrobble' : 'unscrobble'}?key=${ids.join(',')}');
  Future<void> setWatchlistAll(List<String> ids, bool on) async => post('/api/watchlist', {'ids': ids, 'watchlist': on});
  Future<void> setWatched(String id, bool watched) async => post('/:/${watched ? 'scrobble' : 'unscrobble'}?key=$id');
  Future<void> setFavorite(String id, bool on) async => put('/api/items/$id/favorite', {'favorite': on});
  Future<void> setWatchlist(String id, bool on) async => put('/api/items/$id/watchlist', {'watchlist': on});

  /// Your own rating on the server's 0-10 scale; -1 removes it.
  Future<void> rate(String id, int value) async => post('/:/rate?key=$id&rating=$value');

  Future<dynamic> delete(String path) => _send('DELETE', path, null);

  /// Your watchlist with the library's filters; [q] as in [list].
  /// The filters of the library menu with the values found on the watchlist.
  Future<List<Map<String, dynamic>>> watchlistFilters({String type = ''}) async =>
      ((await get('/api/watchlist/filters?includeAdvanced=1${type.isEmpty ? '' : '&type=$type'}'))['MediaContainer']?['Filter'] as List? ?? const [])
          .map((e) => Map<String, dynamic>.from(e))
          .toList();

  Future<List<Map<String, dynamic>>> watchlist(Map<String, String> q) async => metadata(await get('/api/watchlist?${query(q)}'));

  Future<List<Map<String, dynamic>>> playlists({String owner = '', String kind = '', String sort = 'pinned:desc,title', String section = ''}) async =>
      metadata(await get('/playlists?${query({if (owner.isNotEmpty) 'owner': owner, if (kind.isNotEmpty) 'kind': kind, if (section.isNotEmpty) 'section': section, 'sort': sort})}'));

  Future<Map<String, dynamic>> playlist(String id) async => metadata(await get('/playlists/$id')).first;
  Future<List<Map<String, dynamic>>> playlistItems(String id) async => metadata(await get('/playlists/$id/items'));
  Future<Map<String, dynamic>> createPlaylist(String title, {List<String> ids = const []}) async =>
      metadata(await post('/playlists', {'title': title, 'kind': 'manual', if (ids.isNotEmpty) 'ids': ids})).first;
  Future<bool> addToPlaylist(String id, String itemId, {List<String>? ids}) async {
    final r = await post('/playlists/$id/items', {'ids': ids ?? [itemId]});
    return r is Map ? (r['added'] ?? 1) != 0 : true;
  }
  Future<void> updatePlaylist(String id, Map<String, dynamic> patch) async => put('/playlists/$id', patch);
  Future<void> deletePlaylist(String id) async => delete('/playlists/$id');
  Future<void> removeFromPlaylist(String id, String entry) async => delete('/playlists/$id/items/$entry');
  Future<void> moveInPlaylist(String id, String entry, Object after) async => put('/playlists/$id/items/$entry/move?after=$after');

  /// Collections (film series, own ones) or categories (genres) of a library as tiles with title and childCount.
  Future<List<Map<String, dynamic>>> sectionGroups(String sectionId, String kind) async =>
      metadata(await get('/library/sections/$sectionId/$kind'));

  Future<List<Map<String, dynamic>>> search(String q) async =>
      metadata(await get('/hubs/search?query=${Uri.encodeQueryComponent(q)}'));

  /// Image.network cannot send our header, so pictures carry the token in the URL.
  String img(String? path) => path == null || path.isEmpty ? '' : abs(path);

  String abs(String path) => path.startsWith('http') ? path : '$base$path${path.contains('?') ? '&' : '?'}token=$token';

  /// The server's playback decision: direct file URL, HLS URL, resume position (ms).
  Future<Map<String, dynamic>> decision(String id, {String quality = 'original', int? audio}) async =>
      Map<String, dynamic>.from(await get('/library/metadata/$id/decision?native=1&quality=${Uri.encodeQueryComponent(quality)}${audio == null ? '' : '&audio=$audio'}'));

  /// Reports the position. The answer says `stopped: true` (and a `message`) when the owner ended the playback.
  Future<Map<String, dynamic>?> timeline(String id, int ms, String state) async {
    try {
      final r = await post('/:/timeline?ratingKey=$id&time=$ms&state=$state&client=app');
      return r is Map ? Map<String, dynamic>.from(r) : null;
    } catch (e) {
      debugPrint('Reporting the position to $base failed: $e');
      return null;
    }
  }

  Future<void> scrobble(String id) async {
    try {
      await post('/:/scrobble?key=$id');
    } catch (e) {
      debugPrint('Marking as watched at $base failed: $e');
    }
  }
}
