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
}
