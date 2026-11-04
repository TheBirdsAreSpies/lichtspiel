import 'dart:convert';
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

  static Future<Api?> restore() async {
    final p = await SharedPreferences.getInstance();
    final b = p.getString('server');
    final t = p.getString('token');
    if (b == null || t == null) return null;
    return Api(b, t);
  }

  Future<void> save() async {
    final p = await SharedPreferences.getInstance();
    await p.setString('server', base);
    if (token != null) await p.setString('token', token!);
  }

  Future<void> forget() async {
    final p = await SharedPreferences.getInstance();
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

  /// The kinds of filter of a library with their values and counts (genre, year, age rating, unwatched, ...).
  Future<List<Map<String, dynamic>>> filters(String sectionId, {String type = ''}) async =>
      ((await get('/library/sections/$sectionId/filters${type.isEmpty ? '' : '?type=$type'}'))['MediaContainer']?['Filter'] as List? ?? const [])
          .map((e) => Map<String, dynamic>.from(e))
          .toList();

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
  Future<List<Map<String, dynamic>>> related(String id) async {
    try {
      return ((await get('/library/metadata/$id/related?count=20'))['MediaContainer']?['Hub'] as List? ?? const [])
          .map((e) => Map<String, dynamic>.from(e))
          .toList();
    } catch (_) {
      return const [];
    }
  }

  Future<void> setWatched(String id, bool watched) async => post('/:/${watched ? 'scrobble' : 'unscrobble'}?key=$id');
  Future<void> setFavorite(String id, bool on) async => put('/api/items/$id/favorite', {'favorite': on});
  Future<void> setWatchlist(String id, bool on) async => put('/api/items/$id/watchlist', {'watchlist': on});

  /// Your own rating on the server's 0-10 scale; -1 removes it.
  Future<void> rate(String id, int value) async => post('/:/rate?key=$id&rating=$value');

  Future<List<Map<String, dynamic>>> search(String q) async =>
      metadata(await get('/hubs/search?query=${Uri.encodeQueryComponent(q)}'));

  /// Image.network cannot send our header, so pictures carry the token in the URL.
  String img(String? path) => path == null || path.isEmpty ? '' : abs(path);

  String abs(String path) => path.startsWith('http') ? path : '$base$path${path.contains('?') ? '&' : '?'}token=$token';

  /// The server's playback decision: direct file URL, HLS URL, resume position (ms).
  Future<Map<String, dynamic>> decision(String id) async => Map<String, dynamic>.from(await get('/library/metadata/$id/decision'));

  Future<void> timeline(String id, int ms, String state) async {
    try {
      await post('/:/timeline?ratingKey=$id&time=$ms&state=$state&client=app');
    } catch (_) {}
  }

  Future<void> scrobble(String id) async {
    try {
      await post('/:/scrobble?key=$id');
    } catch (_) {}
  }
}
