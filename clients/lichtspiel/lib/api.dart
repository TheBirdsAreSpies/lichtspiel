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

  Future<Map<String, dynamic>> item(String id) async => metadata(await get('/library/metadata/$id')).first;
  Future<List<Map<String, dynamic>>> children(String id) async => metadata(await get('/library/metadata/$id/children'));
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
