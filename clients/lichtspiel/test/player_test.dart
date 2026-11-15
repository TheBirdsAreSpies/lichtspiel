import 'dart:convert';
import 'package:flutter_test/flutter_test.dart';
import 'package:http/http.dart' as http;
import 'package:http/testing.dart';
import 'package:lichtspiel/api.dart';
import 'package:lichtspiel/screens/player.dart';
import 'package:shared_preferences/shared_preferences.dart';

void main() {
  TestWidgetsFlutterBinding.ensureInitialized();

  test('back 10 s near the start stops at 0 instead of going negative', () {
    expect(back10(const Duration(seconds: 4)), Duration.zero);
    expect(back10(const Duration(seconds: 25)), const Duration(seconds: 15));
  });

  test('a choice made in the player goes to the account too, so opening Settings does not undo it', () async {
    SharedPreferences.setMockInitialValues({});
    final sent = <Object?>[];
    await http.runWithClient(() async {
      await savePlayerPref(Api('http://nas:8484', 't'), 'quality', '720p-4');
      await savePlayerPref(Api('http://nas:8484', 't'), 'autoNext', false);
    }, () => MockClient((r) async {
          if (r.url.path == '/api/me/prefs') sent.add(jsonDecode(r.body));
          return http.Response('{}', 200);
        }));
    expect(sent, [
      {'quality': '720p-4'},
      {'autoNext': '0'},
    ]);
    final p = await SharedPreferences.getInstance();
    expect(p.getString('quality'), '720p-4');
    expect(p.getBool('autoNext'), false);
  });
}
