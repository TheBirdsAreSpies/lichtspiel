import 'dart:async';
import 'package:lichtspiel/i18n.dart';

/// Runs before every test file. The widget tests look for the German texts, so they run in German (which
/// also checks that the German pack has them); i18n_test.dart switches languages itself.
Future<void> testExecutable(FutureOr<void> Function() testMain) async {
  I18n.lang.value = 'de';
  await testMain();
}
