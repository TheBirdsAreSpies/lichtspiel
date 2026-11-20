import 'dart:math';
import 'dart:typed_data';
import 'package:flutter_test/flutter_test.dart';
import 'package:image/image.dart' as img;
import 'package:lichtspiel/picture.dart';

/// A picture of noise: it barely compresses, like a detailed photo.
img.Image noise(int w, int h, {bool alpha = false}) {
  final r = Random(1);
  final im = img.Image(width: w, height: h, numChannels: alpha ? 4 : 3);
  for (final p in im) {
    p.setRgba(r.nextInt(256), r.nextInt(256), r.nextInt(256), alpha ? r.nextInt(256) : 255);
  }
  return im;
}

void main() {
  test('a picture that fits is sent as it is', () {
    final small = Uint8List.fromList(img.encodePng(noise(20, 10)));
    final s = shrinkPicture(small, mime: 'image/png', maxBytes: 100000)!;
    expect(s.bytes, same(small));
    expect(s.mime, 'image/png');
  });

  test('a photo too big is made smaller as a JPEG that fits', () {
    final big = Uint8List.fromList(img.encodePng(noise(900, 600)));
    final s = shrinkPicture(big, maxBytes: 120000, maxSide: 800)!;
    expect(s.bytes.length, lessThanOrEqualTo(120000));
    expect(s.mime, 'image/jpeg');
    final back = img.decodeJpg(s.bytes)!;
    expect(back.width, lessThanOrEqualTo(800));
    expect(back.width / back.height, closeTo(1.5, 0.01), reason: 'the shape stays');
  });

  test('a picture with see-through parts (a logo) stays a PNG', () {
    final big = Uint8List.fromList(img.encodePng(noise(400, 200, alpha: true)));
    final s = shrinkPicture(big, mime: 'image/png', maxBytes: 100000)!;
    expect(s.mime, 'image/png');
    expect(s.bytes.length, lessThanOrEqualTo(100000));
    expect(img.decodePng(s.bytes)!.hasAlpha, isTrue);
  });

  test('a camera photo turned by its orientation tag comes out upright', () {
    final photo = noise(300, 200)..exif.imageIfd.orientation = 6; // to be turned 90°
    final bytes = Uint8List.fromList(img.encodeJpg(photo, quality: 100));
    final s = shrinkPicture(bytes, maxBytes: bytes.length - 1)!;
    final back = img.decodeJpg(s.bytes)!;
    expect(back.height, greaterThan(back.width));
  });

  test('something that is no picture gives null', () {
    expect(shrinkPicture(Uint8List.fromList(List.filled(5000, 7)), maxBytes: 100), isNull);
  });
}
