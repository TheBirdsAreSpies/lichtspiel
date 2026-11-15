import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:http/http.dart' as http;
import 'package:http/testing.dart';
import 'package:lichtspiel/api.dart';
import 'package:lichtspiel/screens/playlists.dart';

void main() {
  // A server that refuses everything, e.g. while it restarts.
  final api = Api('http://nas:8484', 't');
  MockClient refusing() => MockClient((_) async => http.Response('{"error":"Server startet neu"}', 503));

  Future<void> tapAndSee(WidgetTester tester, Future<void> Function(BuildContext) action) async {
    await http.runWithClient(() async {
      await tester.pumpWidget(MaterialApp(
        home: Scaffold(body: Builder(builder: (c) => TextButton(onPressed: () => action(c), child: const Text('los')))),
      ));
      await tester.tap(find.text('los'));
      await tester.pump();
      await tester.pump();
    }, refusing);
    expect(find.text('Server startet neu'), findsOneWidget);
  }

  testWidgets('playing a playlist the server refuses says why', (tester) => tapAndSee(tester, (c) => playPlaylist(c, api, '1')));

  testWidgets('"add to playlist" says why when the list cannot be loaded', (tester) => tapAndSee(tester, (c) => addToPlaylistDialog(c, api, '5')));
}
