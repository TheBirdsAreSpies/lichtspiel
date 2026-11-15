import 'dart:convert';
import 'package:flutter_secure_storage/flutter_secure_storage.dart';
import 'package:http/http.dart' as http;
import 'package:shared_preferences/shared_preferences.dart';

class ApiError implements Exception {
  final int status;
  final String message;
  final bool needsCode;
  ApiError(this.status, this.message, {this.needsCode = false});
  @override
  String toString() => message;
}

/// Talks to a Lichtspiel server: token login, JSON requests, stream/picture URLs, progress reports.
class Api {
  String base; // e.g. https://nas:8484
  String? token;
  Api(this.base, [this.token]);

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

  /// "nas" -> "http://nas:8484"; keeps an explicit scheme and port.
  static String normalize(String s) {
    s = s.trim().replaceAll(RegExp(r'/+$'), '');
    if (!s.startsWith('http://') && !s.startsWith('https://')) s = 'http://$s';
    final host = s.replaceFirst(RegExp(r'^https?://'), '');
    if (!host.contains(':')) s = '$s:8484';
    return s;
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
      final msg = body is Map && body['error'] != null ? '${body['error']}' : 'Fehler ${r.statusCode}';
      throw ApiError(r.statusCode, msg, needsCode: body is Map && body['needs_code'] == true);
    }
    return body;
  }

  Future<dynamic> get(String path) async => _decode(await http.get(Uri.parse('$base$path'), headers: _headers));

  Future<dynamic> post(String path, [Object? body]) async =>
      _decode(await http.post(Uri.parse('$base$path'), headers: _headers, body: body == null ? null : jsonEncode(body)));

  Future<dynamic> put(String path, [Object? body]) async =>
      _decode(await http.put(Uri.parse('$base$path'), headers: _headers, body: body == null ? null : jsonEncode(body)));

  /// Any other method (PATCH, ...) with a JSON body.
  Future<dynamic> request(String method, String path, [Object? body]) async {
    final req = http.Request(method, Uri.parse('$base$path'))..headers.addAll(_headers);
    if (body != null) req.body = jsonEncode(body);
    return _decode(await http.Response.fromStream(await req.send()));
  }

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
    } catch (_) {
      return const [];
    }
  }

  /// "Other editions", "More by the director", ... as hubs with a title and Metadata.
  Future<List<Map<String, dynamic>>> related(String id, {int count = 20}) async {
    try {
      return ((await get('/library/metadata/$id/related?count=$count'))['MediaContainer']?['Hub'] as List? ?? const [])
          .map((e) => Map<String, dynamic>.from(e))
          .toList();
    } catch (_) {
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

  Future<dynamic> delete(String path) async => _decode(await http.delete(Uri.parse('$base$path'), headers: _headers));

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
    } catch (_) {
      return null;
    }
  }

  Future<void> scrobble(String id) async {
    try {
      await post('/:/scrobble?key=$id');
    } catch (_) {}
  }
}
