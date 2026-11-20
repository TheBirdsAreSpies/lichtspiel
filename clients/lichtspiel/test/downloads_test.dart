import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:http/http.dart' as http;
import 'package:http/testing.dart';
import 'package:lichtspiel/api.dart';
import 'package:lichtspiel/screens/downloads.dart';

void main() {
  testWidgets('a download that fails right away closes its dialog', (tester) async {
    await http.runWithClient(() async {
      await tester.pumpWidget(MaterialApp(
        home: Scaffold(body: Builder(builder: (c) => TextButton(onPressed: () => downloadToFile(c, Api('http://nas:8484', 't'), '/api/x'), child: const Text('los')))),
      ));
      await tester.tap(find.text('los'));
      await tester.pump();
      await tester.runAsync(() => Future.delayed(const Duration(milliseconds: 500)));
      for (var i = 0; i < 5; i++) {
        await tester.pump(const Duration(milliseconds: 100));
      }
    }, () => MockClient((_) async => throw http.ClientException('Verbindung abgelehnt')));
    expect(find.byType(AlertDialog), findsNothing);
    expect(find.textContaining('Download fehlgeschlagen'), findsOneWidget);
  });
}
