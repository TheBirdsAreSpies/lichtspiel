import 'package:flutter_test/flutter_test.dart';
import 'package:lichtspiel/screens/player.dart';

void main() {
  test('back 10 s near the start stops at 0 instead of going negative', () {
    expect(back10(const Duration(seconds: 4)), Duration.zero);
    expect(back10(const Duration(seconds: 25)), const Duration(seconds: 15));
  });
}
