import 'package:flutter_test/flutter_test.dart';
import 'package:lichtspiel/screens/downloads.dart' show dlQualities;
import 'package:lichtspiel/screens/player.dart';

// A copy of the names in QUALITIES in src/transcode.rs. The server treats any other name as
// "convert without a size or bitrate limit", so a name missing there converts every video at full size.
const serverQualities = ['original', '1080p-20', '1080p-12', '1080p-8', '720p-4', '720p-3', '720p-2', '480p-1.5', '320p-0.7'];

void main() {
  test('the player and Settings offer exactly the server quality names', () {
    expect(playerQualities.keys.toList(), serverQualities);
  });

  test('downloads only offer quality names the server knows', () {
    expect(serverQualities, containsAll(dlQualities.keys));
  });

  test('qualities saved by older versions become server names', () {
    expect(qualityKey('2160'), 'original');
    expect(qualityKey('1080'), '1080p-20');
    expect(qualityKey('720'), '720p-4');
    expect(qualityKey('480'), '480p-1.5');
    expect(qualityKey('360'), '320p-0.7');
    expect(qualityKey('720p-3'), '720p-3');
    expect(qualityKey('nonsense'), 'original');
    expect(qualityKey(null), 'original');
  });

  test('subtitle sizes are the web player ones (SUB_SIZES in web/player.js)', () {
    expect(playerSubSizes.keys.toList(), [75, 100, 130, 170]);
  });

  test('a subtitle size saved by an older version becomes the nearest one offered', () {
    expect(subSizeKey(125), 130);
    expect(subSizeKey(150), 130);
    expect(subSizeKey(200), 170);
    expect(subSizeKey(75), 75);
    expect(subSizeKey(null), 100);
  });
}
