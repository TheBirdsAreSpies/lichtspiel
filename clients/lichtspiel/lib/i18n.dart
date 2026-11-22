import 'package:flutter/foundation.dart';
import 'package:shared_preferences/shared_preferences.dart';
import 'lang/de.dart';

/// The app's languages, like the web app (web/i18n.js): the English text in the code is the key, and
/// tr('Sign in') gives the chosen language's text, or the English itself when the pack has none.
/// {name} in a text is filled from the second argument: tr('Scanning {name}', {'name': 'Movies'}).
/// A key may end in "@@context" when one English word needs two translations ("days@@ago"); the context
/// is never shown.
///
/// The choice is kept on the device and with the account (pref "lang", shared with the web app); without
/// one the device's language is used when there is a pack for it.
class I18n {
  /// Language code → its name in that language.
  static const names = {'en': 'English', 'de': 'Deutsch'};
  static const _packs = <String, Map<String, String>>{'de': deStrings};

  static final lang = ValueNotifier<String>('en');

  static String _pick(Object? code) {
    final c = '${code ?? ''}'.toLowerCase().split(RegExp('[-_]')).first;
    return names.containsKey(c) ? c : '';
  }

  /// The device's language, or English.
  static String get deviceLang {
    for (final l in PlatformDispatcher.instance.locales) {
      final c = _pick(l.languageCode);
      if (c.isNotEmpty) return c;
    }
    return 'en';
  }

  /// Reads the choice saved on this device (before the first screen).
  static Future<void> load() async {
    String saved = '';
    try {
      saved = _pick((await SharedPreferences.getInstance()).getString('lang'));
    } catch (_) {}
    lang.value = saved.isNotEmpty ? saved : deviceLang;
  }

  /// A new choice: shown at once and kept on the device. (The caller sends it to the account.)
  static Future<void> set(String code) async {
    final c = _pick(code);
    if (c.isEmpty) return;
    lang.value = c;
    try {
      await (await SharedPreferences.getInstance()).setString('lang', c);
    } catch (_) {}
  }

  /// The account's choice (from GET /api/me) wins over the device's, like the theme.
  static Future<void> adopt(Object? me) async {
    final prefs = me is Map ? me['prefs'] : null;
    if (prefs is Map && _pick(prefs['lang']).isNotEmpty) await set('${prefs['lang']}');
  }

  /// Texts the server sends (error messages) that carry a number.
  static final _patterns = <String, List<(RegExp, String)>>{
    'de': [
      (RegExp(r'^Too many wrong attempts; try again in (\d+) minute\(s\)$'), r'Zu viele falsche Versuche – versuch es in $1 Minute(n) noch einmal'),
    ],
  };

  static String translate(String en) {
    final pack = _packs[lang.value];
    final hit = pack?[en];
    if (hit != null) return hit;
    for (final (re, to) in _patterns[lang.value] ?? const <(RegExp, String)>[]) {
      final m = re.firstMatch(en);
      if (m != null) return to.replaceAllMapped(RegExp(r'\$(\d)'), (g) => m[int.parse(g[1]!)] ?? '');
    }
    final at = en.indexOf('@@');
    return at < 0 ? en : en.substring(0, at);
  }
}

/// The text in the chosen language, with {name} filled from [args].
String tr(String en, [Map<String, Object?>? args]) {
  final s = I18n.translate(en);
  if (args == null) return s;
  return s.replaceAllMapped(RegExp(r'\{(\w+)\}'), (m) => args.containsKey(m[1]) ? '${args[m[1]]}' : m[0]!);
}

/// A number written with the language's decimal mark ("1.5" → "1,5" in German).
String decimal(Object n) => I18n.lang.value == 'de' ? '$n'.replaceAll('.', ',') : '$n';

/// A day the way the language writes it: 22.11.2026 or 11/22/2026; without [year] 22.11. or 11/22.
/// [pad] writes day and month with two digits.
String fmtDay(DateTime d, {bool year = true, bool pad = false}) {
  String two(int n) => pad ? n.toString().padLeft(2, '0') : '$n';
  if (I18n.lang.value == 'de') return '${two(d.day)}.${two(d.month)}.${year ? d.year : ''}';
  return '${two(d.month)}/${two(d.day)}${year ? '/${d.year}' : ''}';
}
