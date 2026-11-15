import 'dart:convert';
import 'dart:io';
import 'package:http/http.dart' as http;

/// A server that answered on the home network.
class FoundServer {
  final String name;
  final String address; // e.g. http://192.168.1.20:8484
  final String id; // the server's machine id, so one server answering twice is listed once
  const FoundServer(this.name, this.address, this.id);
}

const _searchPort = 32414;
final _group = InternetAddress('239.0.0.250');

/// Reads a "G'Day Mate" answer ("HTTP/1.0 200 OK" or "HELLO * HTTP/1.0" plus "Name: …" lines).
/// Returns null when the packet is not a media server's answer.
FoundServer? parseGdmReply(String packet, String ip) {
  final lines = packet.split(RegExp(r'\r?\n'));
  final first = lines.first;
  if (!((first.startsWith('HTTP/1.') && first.contains(' 200')) || first.startsWith('HELLO'))) return null;
  final h = <String, String>{};
  for (final l in lines.skip(1)) {
    final i = l.indexOf(':');
    if (i > 0) h[l.substring(0, i).trim().toLowerCase()] = l.substring(i + 1).trim();
  }
  if (h['content-type'] != 'plex/media-server') return null;
  final port = int.tryParse(h['port'] ?? '');
  if (port == null) return null;
  final host = ip.contains(':') ? '[$ip]' : ip;
  return FoundServer(h['name'] ?? ip, 'http://$host:$port', h['resource-identifier'] ?? '$ip:$port');
}

/// Asks the home network for servers (Plex-style discovery) and lists the Lichtspiel servers that answer.
/// Never throws: a network that blocks broadcasts, a missing permission or no Wi-Fi just gives an empty list.
Future<List<FoundServer>> discoverServers({Duration wait = const Duration(seconds: 2)}) async {
  final found = <String, FoundServer>{};
  RawDatagramSocket? sock;
  try {
    sock = await RawDatagramSocket.bind(InternetAddress.anyIPv4, 0);
    sock.broadcastEnabled = true;
    final msg = utf8.encode('M-SEARCH * HTTP/1.1\r\n\r\n');
    for (final to in [_group, InternetAddress('255.255.255.255')]) {
      try {
        sock.send(msg, to, _searchPort);
      } catch (_) {}
    }
    sock.listen((ev) {
      if (ev != RawSocketEvent.read) return;
      final d = sock?.receive();
      if (d == null) return;
      final s = parseGdmReply(utf8.decode(d.data, allowMalformed: true), d.address.address);
      if (s != null) found.putIfAbsent(s.id, () => s);
    }, onError: (_) {}, cancelOnError: false);
    await Future<void>.delayed(wait);
  } catch (_) {
    // Discovery is a convenience; typing the address still works.
  } finally {
    sock?.close();
  }
  // A Plex server answers the same search, so keep only the ones that speak Lichtspiel's API.
  final checked = await Future.wait(found.values.map(_check));
  final byId = {for (final s in checked.whereType<FoundServer>()) s.id: s};
  return byId.values.toList()..sort((a, b) => a.name.toLowerCase().compareTo(b.name.toLowerCase()));
}

/// Tries http and then https on the announced port; returns the server with the address that answered.
Future<FoundServer?> _check(FoundServer s) async {
  for (final address in [s.address, s.address.replaceFirst('http://', 'https://')]) {
    try {
      final r = await http.get(Uri.parse('$address/api/status')).timeout(const Duration(seconds: 2));
      if (r.statusCode != 200) continue;
      final j = jsonDecode(utf8.decode(r.bodyBytes));
      if (j is Map && j['machine_id'] != null) return FoundServer('${j['name'] ?? s.name}', address, '${j['machine_id']}');
    } catch (_) {}
  }
  return null;
}
