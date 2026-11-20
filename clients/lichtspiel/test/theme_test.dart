import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:shared_preferences/shared_preferences.dart';
import 'package:lichtspiel/theme.dart';

void main() {
  setUp(() => SharedPreferences.setMockInitialValues({}));

  test('the choice is read from and written as the words the web app uses', () {
    expect(AppTheme.parse('light'), ThemeMode.light);
    expect(AppTheme.parse('dark'), ThemeMode.dark);
    expect(AppTheme.parse('system'), ThemeMode.system);
    expect(AppTheme.parse(null), ThemeMode.system);
    expect(AppTheme.parse('nonsense'), ThemeMode.system);
    for (final m in ThemeMode.values) {
      expect(AppTheme.parse(AppTheme.keyOf(m)), m);
    }
  });

  test('a choice is kept on the device and the account\'s choice wins', () async {
    await AppTheme.set(ThemeMode.light);
    AppTheme.mode.value = ThemeMode.system;
    await AppTheme.load();
    expect(AppTheme.mode.value, ThemeMode.light);

    await AppTheme.adopt({'prefs': {'theme': 'dark'}});
    expect(AppTheme.mode.value, ThemeMode.dark);
    await AppTheme.adopt({'prefs': {}});
    expect(AppTheme.mode.value, ThemeMode.dark);
  });

  testWidgets('text and lines follow the theme', (tester) async {
    late BuildContext light, dark;
    await tester.pumpWidget(Column(children: [
      Theme(data: AppTheme.light, child: Builder(builder: (c) { light = c; return const SizedBox(); })),
      Theme(data: AppTheme.dark, child: Builder(builder: (c) { dark = c; return const SizedBox(); })),
    ]));
    expect(dark.dim, Colors.white60);
    expect(light.dim, Colors.black54);
    expect(light.isDark, isFalse);
    expect(dark.isDark, isTrue);
  });
}
