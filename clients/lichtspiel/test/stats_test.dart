import 'package:flutter_test/flutter_test.dart';
import 'package:lichtspiel/screens/stats.dart';

void main() {
  test('a month goes by day, a year by week from Monday, everything by month', () {
    final now = DateTime(2026, 11, 22, 15);
    final days = statBuckets(now, 30, 30);
    expect(days.length, 30);
    expect(days.last.label, '22.11.');
    final weeks = statBuckets(now, 365, 365);
    expect(DateTime.fromMillisecondsSinceEpoch((weeks.first.from * 1000).toInt()).weekday, DateTime.monday);
    expect(weeks.last.to * 1000, greaterThan(DateTime(2026, 11, 22).millisecondsSinceEpoch));
    // Viewing only since three years ago: months from then, not ten years of empty ones.
    final months = statBuckets(now, 3660, 3 * 365);
    expect(months.first.label, '11/23');
    expect(months.last.label, '11/26');
  });
}
