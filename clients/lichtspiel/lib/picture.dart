import 'dart:typed_data';
import 'package:image/image.dart' as img;

/// What the server takes for an uploaded picture: it reads at most 4 MiB of request, about 3 MB of
/// picture once base64 has made it a third bigger.
const maxPictureBytes = 3 * 1024 * 1024;

/// A picture made small enough to upload, with its MIME type.
typedef Shrunk = ({Uint8List bytes, String mime});

/// Makes a picture fit in [maxBytes] (on a computer the picker can't shrink it). Pictures that fit
/// come back as they are. Others are turned upright (a camera's orientation tag), brought down to
/// [maxSide] pixels on the longer side and saved as JPEG, at a lower quality and then a smaller size
/// until they fit; a picture with see-through parts (a logo) stays a PNG and only gets smaller.
/// Null when the picture can't be read or shrunk. Runs a while for a big photo: call it via `compute`.
Shrunk? shrinkPicture(Uint8List bytes, {String mime = 'image/jpeg', int maxBytes = maxPictureBytes, int maxSide = 3000}) {
  if (bytes.length <= maxBytes) return (bytes: bytes, mime: mime);
  final decoded = img.decodeImage(bytes);
  if (decoded == null) return null;
  final upright = img.bakeOrientation(decoded);
  final png = upright.hasAlpha;
  var side = maxSide;
  while (side >= 200) {
    final longer = upright.width > upright.height ? upright.width : upright.height;
    final sized = longer <= side ? upright : (upright.width >= upright.height ? img.copyResize(upright, width: side, interpolation: img.Interpolation.average) : img.copyResize(upright, height: side, interpolation: img.Interpolation.average));
    if (png) {
      final out = img.encodePng(sized, level: 9);
      if (out.length <= maxBytes) return (bytes: out, mime: 'image/png');
    } else {
      for (final q in const [90, 80, 70]) {
        final out = img.encodeJpg(sized, quality: q);
        if (out.length <= maxBytes) return (bytes: out, mime: 'image/jpeg');
      }
    }
    side = side * 3 ~/ 4;
  }
  return null;
}
