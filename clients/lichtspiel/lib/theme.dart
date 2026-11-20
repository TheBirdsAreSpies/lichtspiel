import 'package:flutter/material.dart';
import 'package:shared_preferences/shared_preferences.dart';

/// Dark or light, like the web app: "system" (the default) follows the device, the choice is kept on the
/// device and with the account (pref "theme"), so the web app and this app agree.
class AppTheme {
  static const seed = Color(0xFFE5A00D);
  static const darkBg = Color(0xFF121212);
  static const lightBg = Color(0xFFF4F5F7);

  static final mode = ValueNotifier<ThemeMode>(ThemeMode.system);

  static String keyOf(ThemeMode m) => switch (m) { ThemeMode.light => 'light', ThemeMode.dark => 'dark', ThemeMode.system => 'system' };
  static ThemeMode parse(Object? s) => switch ('$s') { 'light' => ThemeMode.light, 'dark' => ThemeMode.dark, _ => ThemeMode.system };

  /// Reads the choice saved on this device (before the first screen).
  static Future<void> load() async {
    try {
      mode.value = parse((await SharedPreferences.getInstance()).getString('theme'));
    } catch (_) {}
  }

  /// A new choice: shown at once and kept on the device. (The caller sends it to the account.)
  static Future<void> set(ThemeMode m) async {
    mode.value = m;
    try {
      await (await SharedPreferences.getInstance()).setString('theme', keyOf(m));
    } catch (_) {}
  }

  /// The account's choice (from GET /api/me) wins over the one on the device, like the other settings.
  static Future<void> adopt(Object? me) async {
    final prefs = me is Map ? me['prefs'] : null;
    if (prefs is Map && prefs['theme'] != null) await set(parse(prefs['theme']));
  }

  static ThemeData get dark => ThemeData.dark(useMaterial3: true).copyWith(
        colorScheme: ColorScheme.fromSeed(seedColor: seed, brightness: Brightness.dark),
        scaffoldBackgroundColor: darkBg,
      );

  static ThemeData get light => ThemeData.light(useMaterial3: true).copyWith(
        colorScheme: ColorScheme.fromSeed(seedColor: seed, brightness: Brightness.light),
        scaffoldBackgroundColor: lightBg,
      );
}

/// The colours the screens used to write as Colors.white54 and so on, for either theme.
/// (They read the theme, so a screen follows when it changes.)
extension Palette on BuildContext {
  bool get isDark => Theme.of(this).brightness == Brightness.dark;

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
