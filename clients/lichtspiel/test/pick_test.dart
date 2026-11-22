import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:http/http.dart' as http;
import 'package:http/testing.dart';
import 'package:lichtspiel/api.dart';
import 'package:lichtspiel/widgets/pick.dart';
import 'package:shared_preferences/shared_preferences.dart';

void main() {
  test('"Etwas Kurzes" shows the time left, and an episode keeps its number', () {
    expect(shortCaption({'type': 'movie', 'duration': 90 * 60000}), '1 h 30 min');
    expect(shortCaption({'type': 'movie', 'duration': 90 * 60000, 'viewOffset': 70 * 60000}), 'noch 20 min');
    expect(shortCaption({'type': 'episode', 'parentIndex': 2, 'index': 5, 'duration': 25 * 60000}), 'S2E5 · 25 min');
  });

  testWidgets('"Etwas Kurzes" asks for the chosen time and says when nothing fits', (tester) async {
    SharedPreferences.setMockInitialValues({'shortMinutes': 20});
    final asked = <String>[];
    final api = Api('http://nas:8484', 't');
    await http.runWithClient(() async {
      await tester.pumpWidget(MaterialApp(
        home: Scaffold(body: Builder(builder: (c) => TextButton(onPressed: () => shortDialog(c, api), child: const Text('los')))),
      ));
      await tester.tap(find.text('los'));
      for (var i = 0; i < 5; i++) {
        await tester.pump(const Duration(milliseconds: 50));
      }
      expect(find.text('Nichts, was du gerade schaust oder auf deiner Merkliste hast, passt in 20 Minuten.'), findsOneWidget);
      await tester.tap(find.text('45 min'));
      for (var i = 0; i < 5; i++) {
        await tester.pump(const Duration(milliseconds: 50));
      }
      expect(find.textContaining('passt in 45 Minuten.'), findsOneWidget);
    }, () => MockClient((r) async {
      asked.add(r.url.toString());
      return http.Response('{"MediaContainer":{"size":0,"Metadata":[]}}', 200, headers: {'content-type': 'application/json'});
    }));
    expect(asked, ['http://nas:8484/api/short?minutes=20', 'http://nas:8484/api/short?minutes=45']);
  });
}
