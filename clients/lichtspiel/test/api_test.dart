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

  test('an IPv6 address typed without brackets gets them', () {
    expect(Api.normalize('fd00::5'), 'http://[fd00::5]:8484');
    expect(Api.normalize('https://fe80::1:2:3/'), 'https://[fe80::1:2:3]:8484');
    expect(Api.normalize('2001:db8::7/web'), 'http://[2001:db8::7]:8484/web');
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

    test('an https address on a server that never finishes the handshake gives up with a timeout', () async {
      final s = await silent();
      addTearDown(s.server.close);
      final watch = Stopwatch()..start();
      expect(await failure(Api('https://127.0.0.1:${s.port}').get('/api/status')), 'Server antwortet nicht (Zeitüberschreitung)');
      expect(watch.elapsed, lessThan(const Duration(seconds: 3)));
      // dart:io can't close a socket in the middle of a TLS handshake, so that one connection stays open (see Api._client).
    });

    test('a port nobody listens on says the connection was refused', () async {
      final s = await ServerSocket.bind(InternetAddress.loopbackIPv4, 0);
      final port = s.port;
      await s.close();
      Api.timeout = const Duration(seconds: 10); // Windows needs over a second to refuse a connection: longer than this group's short timeout
      expect(await failure(Api('http://127.0.0.1:$port').get('/api/status')), 'Verbindung abgelehnt – läuft der Server?');
    });

    test('errors get plain German words', () {
      expect(Api.describe(TimeoutException('x')), 'Server antwortet nicht (Zeitüberschreitung)');
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

    test('a timeout on https says the server does not answer, not that it lacks https', () {
      expect(Api.describe(TimeoutException('x'), https: true), 'Server antwortet nicht (Zeitüberschreitung)');
      expect(Api.describe(http.RequestAbortedException(), https: true), 'Server antwortet nicht (Zeitüberschreitung)');
    });

    test('only home-network addresses fall back from https to http', () async {
      // Every https try fails the handshake, every http try answers.
      final tried = <String>[];
      await http.runWithClient(() async {
        for (final host in ['192.168.1.20', '10.0.0.5', '172.20.1.1', '169.254.3.4', '[fd00::5]', '[fe80::1]', 'nas', 'nas.local']) {
          expect((await Api.reach('https://$host:8484')).base, 'http://$host:8484', reason: host);
        }
        for (final host in ['nas.example.com', '8.8.8.8', '172.32.0.1', '[2001:db8::1]']) {
          await expectLater(Api.reach('https://$host:8484'), throwsA(predicate((e) => e is ApiError && e.message == 'Server spricht kein HTTPS – http:// verwenden')), reason: host);
        }
      }, () => MockClient((r) async {
            tried.add('${r.url}');
            if (r.url.isScheme('https')) throw const HandshakeException('WRONG_VERSION_NUMBER');
            return http.Response('{}', 200);
          }));
      expect(tried.where((u) => u.contains('example.com')), ['https://nas.example.com:8484/api/status']);
    });

    /// A server that sends the head of an answer at once and its body byte by byte, [pause] apart;
    /// a null [pause] stops after the first byte and waits. [hungUp] completes when the app hangs up.
    Future<({int port, Future<void> hungUp, ServerSocket server})> trickling(Duration? pause) async {
      final server = await ServerSocket.bind(InternetAddress.loopbackIPv4, 0);
      final hungUp = Completer<void>();
      const body = '{"machine_id":"m"}';
      server.listen((s) async {
        s.listen((_) {}, onDone: () { if (!hungUp.isCompleted) hungUp.complete(); }, onError: (_) { if (!hungUp.isCompleted) hungUp.complete(); });
        try {
          s.write('HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: ${body.length}\r\n\r\n');
          for (var i = 0; i < body.length; i++) {
            s.write(body[i]);
            await s.flush();
            if (pause == null) return;
            await Future<void>.delayed(pause);
          }
          await s.close();
        } catch (_) {}
      });
      return (port: server.port, hungUp: hungUp.future, server: server);
    }

    test('a slow answer body is read to the end while it keeps coming, longer than the timeout', () async {
      final s = await trickling(Api.timeout ~/ 5);
      addTearDown(s.server.close);
      final watch = Stopwatch()..start();
      expect((await Api('http://127.0.0.1:${s.port}').status())['machine_id'], 'm');
      expect(watch.elapsed, greaterThan(Api.timeout * 2));
    });

    test('an answer body that stops coming gives up after the timeout and hangs up', () async {
      final s = await trickling(null);
      addTearDown(s.server.close);
      final watch = Stopwatch()..start();
      expect(await failure(Api('http://127.0.0.1:${s.port}').get('/api/status')), 'Server antwortet nicht (Zeitüberschreitung)');
      expect(watch.elapsed, lessThan(const Duration(seconds: 3)));
      await s.hungUp.timeout(const Duration(seconds: 2));
    });

    test('actor details wait for the server\'s online lookup longer than a normal request', () async {
      final server = await HttpServer.bind(InternetAddress.loopbackIPv4, 0);
      addTearDown(() => server.close(force: true));
      server.listen((r) async {
        await Future<void>.delayed(Api.timeout * 2);
        r.response.headers.contentType = ContentType.json;
        r.response.write('{"name":"A"}');
        await r.response.close();
      });
      expect((await Api('http://127.0.0.1:${server.port}').personDetails('k', 'A'))['name'], 'A');
    });

    test('requests to the same server share one connection', () async {
      final server = await HttpServer.bind(InternetAddress.loopbackIPv4, 0);
      addTearDown(() => server.close(force: true));
      final ports = <int>{};
      server.listen((r) {
        ports.add(r.connectionInfo!.remotePort);
        r.response.write('{}');
        r.response.close();
      });
      final api = Api('http://127.0.0.1:${server.port}');
      for (var i = 0; i < 3; i++) {
        await api.get('/api/status');
      }
      expect(ports, hasLength(1));
    });

    test('a request that times out does not stop the next one', () async {
      final s = await silent();
      addTearDown(s.server.close);
      final server = await HttpServer.bind(InternetAddress.loopbackIPv4, 0);
      addTearDown(() => server.close(force: true));
      server.listen((r) {
        r.response.write('{"ok":1}');
        r.response.close();
      });
      final slow = failure(Api('http://127.0.0.1:${s.port}').get('/api/status'));
      expect((await Api('http://127.0.0.1:${server.port}').get('/x'))['ok'], 1);
      expect(await slow, 'Server antwortet nicht (Zeitüberschreitung)');
      await s.closed.timeout(const Duration(seconds: 2));
      expect((await Api('http://127.0.0.1:${server.port}').get('/x'))['ok'], 1);
    });

    test('an http address is never tried with https behind the back', () async {
      final s = await ServerSocket.bind(InternetAddress.loopbackIPv4, 0);
      final port = s.port;
      await s.close();
      Api.timeout = const Duration(seconds: 10); // Windows needs over a second to refuse a connection: longer than this group's short timeout
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
