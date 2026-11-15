import 'package:flutter_secure_storage/flutter_secure_storage.dart';
import 'package:flutter_test/flutter_test.dart';
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
}
