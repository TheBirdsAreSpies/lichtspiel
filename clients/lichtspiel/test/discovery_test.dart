import 'package:flutter_test/flutter_test.dart';
import 'package:lichtspiel/discovery.dart';
import 'package:lichtspiel/screens/downloads.dart' show fileNameFrom;

void main() {
  test('a server answer gives name, address and id', () {
    const reply = 'HTTP/1.0 200 OK\r\nContent-Type: plex/media-server\r\nName: Wohnzimmer\r\nPort: 8484\r\nResource-Identifier: abc123\r\nUpdated-At: 1\r\nVersion: 1.0\r\n';
    final s = parseGdmReply(reply, '192.168.1.20')!;
    expect(s.name, 'Wohnzimmer');
    expect(s.address, 'http://192.168.1.20:8484');
    expect(s.id, 'abc123');
  });

  test('only answers to the search count, other packets do not', () {
    // The server's HELLO goes to the multicast group on 32413, which this socket never joins.
    expect(parseGdmReply('HELLO * HTTP/1.0\r\nContent-Type: plex/media-server\r\nName: NAS\r\nPort: 9000\r\n', '10.0.0.5'), isNull);
    expect(parseGdmReply('M-SEARCH * HTTP/1.1\r\n\r\n', '10.0.0.5'), isNull);
    expect(parseGdmReply('HTTP/1.0 200 OK\r\nContent-Type: plex/media-player\r\nPort: 32500\r\n', '10.0.0.5'), isNull);
    expect(parseGdmReply('HTTP/1.0 200 OK\r\nContent-Type: plex/media-server\r\nName: x\r\n', '10.0.0.5'), isNull);
  });

  test('download file names come from the server header', () {
    expect(fileNameFrom('attachment; filename="Der Film (2020).mkv"; filename*=UTF-8\'\'Der%20Film%20%282020%29.mkv'), 'Der Film (2020).mkv');
    expect(fileNameFrom('attachment; filename="M_de.mp4"; filename*=UTF-8\'\'M%C3%A4dchen.mp4'), 'Mädchen.mp4');
    expect(fileNameFrom('attachment; filename="plain.mp4"'), 'plain.mp4');
    expect(fileNameFrom(null), isNull);
  });
}
