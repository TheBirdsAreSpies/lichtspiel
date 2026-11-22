import 'package:flutter_test/flutter_test.dart';
import 'package:lichtspiel/i18n.dart';
import 'package:lichtspiel/lang/de.dart';
import 'package:shared_preferences/shared_preferences.dart';

void main() {
  setUp(() => SharedPreferences.setMockInitialValues({}));
  tearDown(() => I18n.lang.value = 'de');

  test('English is the text itself, German comes from the pack, {names} are filled in', () {
    I18n.lang.value = 'en';
    expect(tr('Sign in'), 'Sign in');
    expect(tr('{n} movies', {'n': 3}), '3 movies');
    expect(tr('days@@ago'), 'days');
    expect(tr('A text no pack has'), 'A text no pack has');
    I18n.lang.value = 'de';
    expect(tr('Sign in'), 'Anmelden');
    expect(tr('{n} movies', {'n': 3}), '3 Filme');
    expect(tr('days@@ago'), 'Tagen');
    expect(tr('A text no pack has'), 'A text no pack has');
  });

  test('the server\'s error texts are translated, also the ones with a number', () {
    I18n.lang.value = 'de';
    expect(tr('Wrong PIN'), 'Falsche PIN');
    expect(tr('Too many wrong attempts; try again in 5 minute(s)'), 'Zu viele falsche Versuche – versuch es in 5 Minute(n) noch einmal');
    I18n.lang.value = 'en';
    expect(tr('Too many wrong attempts; try again in 5 minute(s)'), 'Too many wrong attempts; try again in 5 minute(s)');
  });

  test('every German text keeps the {names} of its English one', () {
    final names = RegExp(r'\{(\w+)\}');
    for (final e in deStrings.entries) {
      final en = names.allMatches(e.key).map((m) => m[1]).toSet();
      final de = names.allMatches(e.value).map((m) => m[1]).toSet();
      expect(de, en, reason: e.key);
    }
  });

  test('numbers and days are written the way the language does', () {
    final d = DateTime(2026, 3, 7);
    I18n.lang.value = 'de';
    expect(decimal(1.5), '1,5');
    expect(fmtDay(d), '7.3.2026');
    expect(fmtDay(d, year: false, pad: true), '07.03.');
    I18n.lang.value = 'en';
    expect(decimal(1.5), '1.5');
    expect(fmtDay(d), '3/7/2026');
    expect(fmtDay(d, year: false, pad: true), '03/07');
  });

  test('the choice is kept on the device and the account\'s choice wins', () async {
    await I18n.set('en');
    I18n.lang.value = 'de';
    await I18n.load();
    expect(I18n.lang.value, 'en');
    await I18n.adopt({'prefs': {'lang': 'de'}});
    expect(I18n.lang.value, 'de');
    await I18n.adopt({'prefs': {'lang': 'xx'}});
    await I18n.adopt({'prefs': {}});
    expect(I18n.lang.value, 'de');
    await I18n.set('de-AT');
    expect(I18n.lang.value, 'de');
  });
}
