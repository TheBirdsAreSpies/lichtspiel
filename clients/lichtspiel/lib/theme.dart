import 'package:flutter/material.dart';
import 'package:shared_preferences/shared_preferences.dart';

/// Dark, light or pure black (OLED), and the accent colour, like the web app: "system" (the default) follows
/// the device; both choices are kept on the device and with the account (prefs "theme" and "accent"),
/// so the web app and this app agree.
class AppTheme {
  static const darkBg = Color(0xFF121212);
  static const lightBg = Color(0xFFF4F5F7);

  static const choices = ['system', 'light', 'dark', 'oled'];

  /// Accent colours: [on dark, on light]. The first one is the default.
  static const accents = <String, List<Color>>{
    'amber': [Color(0xFFE5A00D), Color(0xFFD98E04)],
    'blue': [Color(0xFF4C9AFF), Color(0xFF1F66C2)],
    'green': [Color(0xFF3ECF75), Color(0xFF1B8A45)],
    'red': [Color(0xFFFF6B61), Color(0xFFC9362E)],
    'violet': [Color(0xFFA78BFA), Color(0xFF6D4AD6)],
  };

  static final choice = ValueNotifier<String>('system');
  static final accent = ValueNotifier<String>('amber');
  static Listenable get changes => Listenable.merge([choice, accent]);

  static String _theme(Object? s) => choices.contains('$s') ? '$s' : 'system';
  static String _accent(Object? s) => accents.containsKey('$s') ? '$s' : 'amber';

  static ThemeMode get mode => switch (choice.value) { 'light' => ThemeMode.light, 'dark' || 'oled' => ThemeMode.dark, _ => ThemeMode.system };

  /// Reads the choices saved on this device (before the first screen).
  static Future<void> load() async {
    try {
      final p = await SharedPreferences.getInstance();
      choice.value = _theme(p.getString('theme'));
      accent.value = _accent(p.getString('accent'));
    } catch (_) {}
  }

  /// New choices: shown at once and kept on the device. (The caller sends them to the account.)
  static Future<void> set({String? theme, String? accentName}) async {
    if (theme != null) choice.value = _theme(theme);
    if (accentName != null) accent.value = _accent(accentName);
    try {
      final p = await SharedPreferences.getInstance();
      if (theme != null) await p.setString('theme', choice.value);
      if (accentName != null) await p.setString('accent', accent.value);
    } catch (_) {}
  }

  /// The account's choices (from GET /api/me) win over the ones on the device, like the other settings.
  static Future<void> adopt(Object? me) async {
    final prefs = me is Map ? me['prefs'] : null;
    if (prefs is Map) await set(theme: prefs['theme'] == null ? null : '${prefs['theme']}', accentName: prefs['accent'] == null ? null : '${prefs['accent']}');
  }

  static Color accentColor(bool dark) => accents[accent.value]![dark ? 0 : 1];

  static ThemeData get dark {
    final oled = choice.value == 'oled';
    return ThemeData.dark(useMaterial3: true).copyWith(
      colorScheme: ColorScheme.fromSeed(seedColor: accentColor(true), brightness: Brightness.dark, surface: oled ? Colors.black : null),
      scaffoldBackgroundColor: oled ? Colors.black : darkBg,
    );
  }

  static ThemeData get light => ThemeData.light(useMaterial3: true).copyWith(
        colorScheme: ColorScheme.fromSeed(seedColor: accentColor(false), brightness: Brightness.light),
        scaffoldBackgroundColor: lightBg,
      );
}

/// The colours the screens used to write as Colors.white54 and so on, for either theme.
/// (They read the theme, so a screen follows when it changes.)
extension Palette on BuildContext {
  bool get isDark => Theme.of(this).brightness == Brightness.dark;

  /// The accent colour (stars, badges, marks) and the colour of what is written on it.
  Color get accent => AppTheme.accentColor(isDark);
  Color get onAccent => accent.computeLuminance() > .4 ? const Color(0xFF16120A) : Colors.white;

  /// Secondary text (was white60), quieter text and icons (white54), the quietest (white38).
  Color get dim => isDark ? Colors.white60 : Colors.black54;
  Color get faint => isDark ? Colors.white54 : Colors.black45;
  Color get ghost => isDark ? Colors.white38 : Colors.black38;

  /// Text just below the main colour (was white70).
  Color get soft => isDark ? Colors.white70 : Colors.black87;

  /// Hairlines and quiet fills (was white10 and white12).
  Color get hair => isDark ? Colors.white10 : Colors.black12;
  Color get hair2 => isDark ? Colors.white12 : Colors.black12;

  /// Meaning colours: bright on dark, deeper on light so they can be read.
  Color get good => isDark ? Colors.greenAccent : Colors.green.shade700;
  Color get bad => isDark ? Colors.redAccent : Colors.red.shade700;
  Color get warn => isDark ? Colors.orangeAccent : Colors.orange.shade800;
  Color get info => isDark ? Colors.lightBlueAccent : Colors.blue.shade700;
  Color get gold => isDark ? Colors.amber : Colors.amber.shade800;
}
