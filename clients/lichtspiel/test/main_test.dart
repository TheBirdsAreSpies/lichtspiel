import 'dart:async';
import 'package:flutter_secure_storage/flutter_secure_storage.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:http/http.dart' as http;
import 'package:http/testing.dart';
import 'package:lichtspiel/api.dart';
import 'package:lichtspiel/main.dart';
import 'package:lichtspiel/screens/home.dart';
import 'package:shared_preferences/shared_preferences.dart';

void main() {
  testWidgets('a saved server that never answers (away from home) does not keep the start spinner up', (tester) async {
    SharedPreferences.setMockInitialValues({'server': 'http://192.168.1.20:8484'});
    FlutterSecureStorage.setMockInitialValues({'token': 't'});
    final never = Completer<http.Response>();
    await http.runWithClient(() async {
      await tester.pumpWidget(const LichtspielApp());
      await tester.pump(const Duration(seconds: 11));
      await tester.pump();
    }, () => MockClient((_) => never.future));
    expect(find.byType(HomeScreen), findsOneWidget);
    await tester.pump(Api.timeout); // the home screen's own requests give up too
  });
}
