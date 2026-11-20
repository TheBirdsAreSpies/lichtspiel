import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:shared_preferences/shared_preferences.dart';
import 'package:lichtspiel/theme.dart';

void main() {
  setUp(() async {
    SharedPreferences.setMockInitialValues({});
    await AppTheme.set(theme: 'system', accentName: 'amber');
  });

  test('OLED is dark with a black page; unknown words fall back to the defaults', () async {
    await AppTheme.set(theme: 'oled');
    expect(AppTheme.mode, ThemeMode.dark);
    expect(AppTheme.dark.scaffoldBackgroundColor, Colors.black);
    await AppTheme.set(theme: 'dark');
    expect(AppTheme.dark.scaffoldBackgroundColor, AppTheme.darkBg);
    await AppTheme.set(theme: 'light');
    expect(AppTheme.mode, ThemeMode.light);
    await AppTheme.set(theme: 'nonsense', accentName: 'pink');
    expect(AppTheme.choice.value, 'system');
    expect(AppTheme.mode, ThemeMode.system);
    expect(AppTheme.accent.value, 'amber');
  });

  test('the choices are kept on the device and the account\'s choices win', () async {
    await AppTheme.set(theme: 'light', accentName: 'blue');
    AppTheme.choice.value = 'system';
    AppTheme.accent.value = 'amber';
    await AppTheme.load();
    expect(AppTheme.choice.value, 'light');
    expect(AppTheme.accent.value, 'blue');

    await AppTheme.adopt({'prefs': {'theme': 'oled', 'accent': 'violet'}});
    expect(AppTheme.choice.value, 'oled');
    expect(AppTheme.accent.value, 'violet');
    await AppTheme.adopt({'prefs': {}});
    expect(AppTheme.choice.value, 'oled');
  });

  testWidgets('text, lines and the accent follow the theme', (tester) async {
    late BuildContext light, dark;
    await tester.pumpWidget(Column(children: [
      Theme(data: AppTheme.light, child: Builder(builder: (c) { light = c; return const SizedBox(); })),
      Theme(data: AppTheme.dark, child: Builder(builder: (c) { dark = c; return const SizedBox(); })),
    ]));
    expect(dark.dim, Colors.white60);
    expect(light.dim, Colors.black54);
    expect(light.isDark, isFalse);
    expect(dark.isDark, isTrue);
    expect(dark.accent, AppTheme.accents['amber']![0]);
    expect(light.accent, AppTheme.accents['amber']![1]);
    expect(dark.onAccent, const Color(0xFF16120A));
    await AppTheme.set(accentName: 'blue');
    expect(light.onAccent, Colors.white);
  });
}
