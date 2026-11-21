import 'package:flutter_test/flutter_test.dart';
import 'package:lichtspiel/screens/calendar.dart';

void main() {
  final today = DateTime(2026, 11, 21); // a Saturday

  test('days near today get a word, later ones the date', () {
    expect(comingDay('2026-11-21', today: today), 'Heute');
    expect(comingDay('2026-11-22', today: today), 'Morgen');
    expect(comingDay('2026-11-25', today: today), 'Mittwoch');
    expect(comingDay('2026-12-06', today: today), 'So., 6.12.');
    expect(comingDay('2026-11-22', long: true, today: today), 'Morgen · Sonntag, 22. November');
    expect(comingDay('2027-03-01', long: true, today: today), 'Montag, 1. März');
    expect(comingDay('kaputt', today: today), 'kaputt');
  });

  test('an episode and a movie are described', () {
    expect(comingWhat({'kind': 'episode', 'season': 4, 'episode': 1, 'name': 'Anfang'}), 'S4 E1 · Anfang');
    expect(comingWhat({'kind': 'movie', 'year': 2026, 'note': 'in cinemas'}), '2026 · Im Kino');
  });

  test('one card a show with the number of episodes that follow, every movie on its own', () {
    final cards = comingCards([
      {'kind': 'episode', 'show_id': 9, 'title': 'The Simpsons', 'date': '2026-11-22'},
      {'kind': 'movie', 'title': 'Dune', 'date': '2026-11-23'},
      {'kind': 'episode', 'show_id': 9, 'title': 'The Simpsons', 'date': '2026-11-29'},
      {'kind': 'episode', 'title': 'Andor', 'date': '2026-11-30'},
      {'kind': 'episode', 'title': 'Andor', 'date': '2026-12-07'},
      {'kind': 'movie', 'title': 'Dune', 'date': '2026-12-20'},
    ]);
    expect(cards.map((c) => '${c.$1['title']} ${c.$2}').toList(), ['The Simpsons 1', 'Dune 0', 'Andor 1', 'Dune 0']);
  });
}
