import 'dart:async';
import 'dart:convert';
import 'dart:io';
import 'package:flutter_secure_storage/flutter_secure_storage.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:http/http.dart' as http;
import 'package:http/testing.dart';
import 'package:lichtspiel/api.dart';
import 'package:shared_preferences/shared_preferences.dart';

void main() {
  TestWidgetsFlutterBinding.ensureInitialized();

  test('server address gets scheme and default port', () {
    expect(Api.normalize('nas'), 'http://nas:8484');
    expect(Api.normalize('192.168.1.20:9000/'), 'http://192.168.1.20:9000');
    expect(Api.normalize('https://nas.example.com:8484'), 'https://nas.example.com:8484');
  });

  test('an IPv6 address without a port gets the default port', () {
    expect(Api.normalize('[fd00::5]'), 'http://[fd00::5]:8484');
    expect(Api.normalize('https://[fd00::5]'), 'https://[fd00::5]:8484');
    expect(Api.normalize('http://[fd00::5]:9000'), 'http://[fd00::5]:9000');
  });

  test('a token from an older version moves into the keychain once', () async {
    SharedPreferences.setMockInitialValues({'server': 'http://nas:8484', 'token': 'old'});
    FlutterSecureStorage.setMockInitialValues({});
    final a = await Api.restore();
    expect(a?.base, 'http://nas:8484');
    expect(a?.token, 'old');
    final p = await SharedPreferences.getInstance();
    expect(p.getString('token'), isNull);
    expect(p.getString('server'), 'http://nas:8484');
    expect(await const FlutterSecureStorage().read(key: 'token'), 'old');
    // Signing out removes it.
    await a!.forget();
    expect(await const FlutterSecureStorage().read(key: 'token'), isNull);
    expect(await Api.restore(), isNull);
  });

  test('a new sign-in is kept in the keychain, not in the settings', () async {
    SharedPreferences.setMockInitialValues({});
    FlutterSecureStorage.setMockInitialValues({});
    await Api('http://nas:8484', 'fresh').save();
    final p = await SharedPreferences.getInstance();
    expect(p.getString('token'), isNull);
    expect((await Api.restore())?.token, 'fresh');
  });

  test('a token in the settings wins over the keychain: it is the newer one', () async {
    // save() only writes the settings when the keychain refused, so that token came later.
    SharedPreferences.setMockInitialValues({'server': 'http://nas:8484', 'token': 'newer'});
    FlutterSecureStorage.setMockInitialValues({'token': 'older'});
    expect((await Api.restore())?.token, 'newer');
    final p = await SharedPreferences.getInstance();
    expect(p.getString('token'), isNull);
    expect(await const FlutterSecureStorage().read(key: 'token'), 'newer');
  });

  test('when the keychain refuses, the token stays in the plain settings', () async {
    SharedPreferences.setMockInitialValues({});
    FlutterSecureStorage.setMockInitialValues(const {}); // a const map: every keychain write throws
    await Api('http://nas:8484', 'fresh').save();
    final p = await SharedPreferences.getInstance();
    expect(p.getString('token'), 'fresh');
    expect((await Api.restore())?.token, 'fresh');
  });

  test('a 401 for a signed-in request reports that the sign-in is gone, a wrong password does not', () async {
    var gone = 0;
    Api.onUnauthorized = (_) => gone++;
    addTearDown(() => Api.onUnauthorized = null);
    await http.runWithClient(() async {
      await expectLater(Api('http://nas:8484', 't').get('/hubs'), throwsA(isA<ApiError>()));
      await expectLater(Api('http://nas:8484').post('/api/login', {}), throwsA(isA<ApiError>()));
    }, () => MockClient((_) async => http.Response('{"error":"Please sign in"}', 401)));
    expect(gone, 1);
  });

  group('connection problems', () {
    late Duration saved;
    HttpOverrides? fake;
    setUp(() {
      saved = Api.timeout;
      Api.timeout = const Duration(milliseconds: 500);
      fake = HttpOverrides.current;
      HttpOverrides.global = null; // the test binding answers every real request with 400; these tests need real sockets
    });
    tearDown(() {
      Api.timeout = saved;
      HttpOverrides.global = fake;
    });

    /// A server that takes the connection and never says anything; [closed] completes when the app hangs up.
    Future<({int port, Future<void> closed, ServerSocket server})> silent() async {
      final server = await ServerSocket.bind(InternetAddress.loopbackIPv4, 0);
      final closed = Completer<void>();
      server.listen((s) => s.listen((_) {}, onDone: () { closed.complete(); s.destroy(); }, onError: (_) => closed.complete()));
      return (port: server.port, closed: closed.future, server: server);
    }

    Future<String> failure(Future<Object?> f) async {
      try {
        await f;
      } on ApiError catch (e) {
        expect(e.status, 0);
        return e.message;
      }
      fail('no error');
    }

    test('a server that never answers gives up after the timeout and hangs up', () async {
      final s = await silent();
      addTearDown(s.server.close);
      final watch = Stopwatch()..start();
      expect(await failure(Api('http://127.0.0.1:${s.port}').get('/api/status')), 'Server antwortet nicht (Zeitüberschreitung)');
      expect(watch.elapsed, lessThan(const Duration(seconds: 3)));
      await s.closed.timeout(const Duration(seconds: 2));
    });

    test('an https address on a server that never finishes the handshake gives up and says to use http', () async {
      final s = await silent();
      addTearDown(s.server.close);
      final watch = Stopwatch()..start();
      expect(await failure(Api('https://127.0.0.1:${s.port}').get('/api/status')), 'Server spricht kein HTTPS – http:// verwenden');
      expect(watch.elapsed, lessThan(const Duration(seconds: 3)));
      // dart:io can't close a socket in the middle of a TLS handshake, so that one connection stays open (see Api._client).
    });

    test('a port nobody listens on says the connection was refused', () async {
      final s = await ServerSocket.bind(InternetAddress.loopbackIPv4, 0);
      final port = s.port;
      await s.close();
      expect(await failure(Api('http://127.0.0.1:$port').get('/api/status')), 'Verbindung abgelehnt – läuft der Server?');
    });

    test('errors get plain German words', () {
      expect(Api.describe(TimeoutException('x')), 'Server antwortet nicht (Zeitüberschreitung)');
      expect(Api.describe(TimeoutException('x'), https: true), 'Server spricht kein HTTPS – http:// verwenden');
      expect(Api.describe(const SocketException('Failed host lookup: \'nas.invalid\'', osError: OSError('nodename nor servname provided, or not known', 8))), 'Adresse nicht gefunden');
      expect(Api.describe(const SocketException('Connection refused', osError: OSError('Connection refused', 61))), 'Verbindung abgelehnt – läuft der Server?');
      expect(Api.describe(const HandshakeException('WRONG_VERSION_NUMBER'), https: true), 'Server spricht kein HTTPS – http:// verwenden');
      expect(Api.describe(const HandshakeException('Handshake error in client (OS Error: CERTIFICATE_VERIFY_FAILED: self signed certificate)'), https: true), contains('Zertifikat'));
      expect(Api.describe(Exception('something else')), 'Server nicht erreichbar');
    });

    test('an https address on a plain-http server is reached over http once', () async {
      final server = await HttpServer.bind(InternetAddress.loopbackIPv4, 0);
      addTearDown(() => server.close(force: true));
      server.listen((r) {
        r.response.headers.contentType = ContentType.json;
        r.response.write(jsonEncode({'machine_id': 'm', 'name': 'NAS'}));
        r.response.close();
      });
      final api = await Api.reach('https://127.0.0.1:${server.port}');
      expect(api.base, 'http://127.0.0.1:${server.port}');
    });

    test('an http address is never tried with https behind the back', () async {
      final s = await ServerSocket.bind(InternetAddress.loopbackIPv4, 0);
      final port = s.port;
      await s.close();
      expect(await failure(Api.reach('http://127.0.0.1:$port')), 'Verbindung abgelehnt – läuft der Server?');
    });
  });

  test('known server errors come in German, unknown ones as they are', () async {
    await http.runWithClient(() async {
      await expectLater(Api('http://nas:8484').post('/api/login', {}), throwsA(predicate((e) => e is ApiError && e.message == 'Benutzername oder Passwort falsch')));
      await expectLater(Api('http://nas:8484').post('/api/home/1/login', {}), throwsA(predicate((e) => e is ApiError && e.message == 'Falsche PIN')));
      await expectLater(Api('http://nas:8484').post('/api/x', {}), throwsA(predicate((e) => e is ApiError && e.message == 'Something new')));
      await expectLater(Api('http://nas:8484').post('/api/lock', {}), throwsA(predicate((e) => e is ApiError && e.message == 'Zu viele falsche Versuche – versuch es in 3 Minute(n) noch einmal')));
    }, () => MockClient((r) async => http.Response(
        jsonEncode({'error': switch (r.url.path) {
          '/api/login' => 'User name or password is wrong',
          '/api/home/1/login' => 'Wrong PIN',
          '/api/lock' => 'Too many wrong attempts; try again in 3 minute(s)',
          _ => 'Something new',
        }}),
        401)));
  });

  test('media and picture addresses never carry the sign-in token', () {
    final api = Api('http://nas:8484', 'secret');
    expect(api.abs('/library/parts/1/file?x=1&ticket=t1'), 'http://nas:8484/library/parts/1/file?x=1&ticket=t1');
    expect(api.img('/library/metadata/1/thumb'), 'http://nas:8484/library/metadata/1/thumb');
    expect(api.abs('/library/parts/1/frame?t=5'), isNot(contains('secret')));
  });

  test('the token goes along as a header to our server only', () {
    final api = Api('http://nas:8484', 'secret');
    expect(api.headersFor(api.img('/library/metadata/1/thumb')), {'X-Token': 'secret'});
    expect(api.headersFor('https://image.tmdb.org/t/p/w185/x.jpg'), isNull);
    expect(Api('http://nas:8484').headersFor('http://nas:8484/library/metadata/1/thumb'), isNull);
    expect(api.headersFor('http://nas:8484.evil.com/x.jpg'), isNull);
    expect(api.headersFor('http://nas:84840/x.jpg'), isNull);
    expect(Api('https://myserver', 'secret').headersFor('https://myserver.evil.com/x.jpg'), isNull);
    expect(Api('https://nas', 'secret').headersFor('https://nas:443/x.jpg'), {'X-Token': 'secret'});
  });

  test('a file for the browser to save gets a ticket for its address instead of the token', () async {
    late http.Request asked;
    final link = await http.runWithClient(() => Api('http://nas:8484', 'secret').ticketLink('/api/log/download?gz=1'), () => MockClient((r) async {
          asked = r;
          return http.Response(jsonEncode({'ticket': 'T1', 'expires': 1}), 200);
        }));
    expect(asked.url.path, '/api/ticket');
    expect(asked.url.queryParameters['path'], '/api/log/download');
    expect(asked.headers['X-Token'], 'secret');
    expect(link, 'http://nas:8484/api/log/download?gz=1&ticket=T1');
  });
}
