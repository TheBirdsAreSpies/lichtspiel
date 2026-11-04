import 'dart:async';
import 'package:flutter/material.dart';
import '../api.dart';
import 'home.dart';

String fmtBits(num bytesPerSec) {
  final b = bytesPerSec * 8;
  if (b >= 1e9) return '${(b / 1e9).toStringAsFixed(1)} Gbit/s';
  if (b >= 1e6) return '${(b / 1e6).toStringAsFixed(b >= 1e7 ? 0 : 1)} Mbit/s';
  if (b >= 1e3) return '${(b / 1e3).toStringAsFixed(0)} kbit/s';
  return '${b.toStringAsFixed(0)} bit/s';
}

String fmtBytes(num b) {
  if (b >= 1e12) return '${(b / 1e12).toStringAsFixed(1)} TB';
  if (b >= 1e9) return '${(b / 1e9).toStringAsFixed(1)} GB';
  if (b >= 1e6) return '${(b / 1e6).toStringAsFixed(0)} MB';
  return '${(b / 1e3).toStringAsFixed(0)} kB';
}

String fmtPct(dynamic v) => v == null ? '–' : '${(v as num).toStringAsFixed(0)} %';

String fmtClockMs(num ms) {
  final s = ms ~/ 1000;
  final h = s ~/ 3600, m = (s % 3600) ~/ 60, sec = s % 60;
  final mm = m.toString().padLeft(2, '0'), ss = sec.toString().padLeft(2, '0');
  return h > 0 ? '$h:$mm:$ss' : '$m:$ss';
}

String fmtUptime(num sec) {
  final d = sec ~/ 86400, h = (sec % 86400) ~/ 3600, m = (sec % 3600) ~/ 60;
  return d > 0 ? '$d d $h h' : h > 0 ? '$h h $m min' : '$m min';
}

String fmtAgo(num at, num now) {
  final d = now - at;
  if (d < 90) return 'gerade eben';
  if (d < 5400) return 'vor ${(d / 60).round()} min';
  if (d < 129600) return 'vor ${(d / 3600).round()} h';
  return 'vor ${(d / 86400).round()} Tagen';
}

String fmtDateTime(num at) {
  final t = DateTime.fromMillisecondsSinceEpoch((at * 1000).round());
  String p(int n) => n.toString().padLeft(2, '0');
  return '${p(t.day)}.${p(t.month)}.${t.year} ${p(t.hour)}:${p(t.minute)}';
}

const _codecs = {'h264': 'H.264', 'hevc': 'HEVC', 'av1': 'AV1', 'vp9': 'VP9', 'mpeg2video': 'MPEG-2', 'aac': 'AAC', 'ac3': 'AC3', 'eac3': 'E-AC3', 'dts': 'DTS', 'truehd': 'TrueHD', 'flac': 'FLAC', 'opus': 'Opus', 'mp3': 'MP3'};
String _codec(dynamic c) => _codecs['$c'] ?? '${c ?? ''}'.toUpperCase();
String _res(dynamic h) {
  final n = (h ?? 0) as num;
  return n == 0 ? '' : n >= 2000 ? '4K' : n >= 1000 ? '1080p' : n >= 700 ? '720p' : n >= 540 ? '576p' : n >= 470 ? '480p' : 'SD';
}

String _ch(dynamic n) => const {1: 'Mono', 2: 'Stereo', 3: '2.1', 6: '5.1', 7: '6.1', 8: '7.1'}[n] ?? (n == null ? '' : '$n ch');

String _videoLine(Map v) {
  final from = '${_res(v['height'])} (${['${_codec(v['codec'])}', if (v['profile'] != null) '${v['profile']}', if (v['hdr'] == true) 'HDR'].join(' ')})';
  return v['action'] == 'transcode' ? '$from → ${_res(v['out_height'])} (${_codec(v['out_codec'])}) — Umwandlung${v['hw'] == true ? ' (Hardware)' : ''}' : '$from — direkt';
}

String _audioLine(Map a) {
  final lang = a['language'] ?? a['title'] ?? 'Unbekannt';
  final from = '$lang (${[_codec(a['codec']), _ch(a['channels'])].where((s) => s.isNotEmpty).join(' ')})';
  return a['action'] == 'transcode' ? '$from → ${_codec(a['out_codec'])} ${_ch(a['out_channels'])} — Umwandlung' : '$from — direkt';
}

const _ranges = {
  'realtime': 'Echtzeit',
  '12h': 'Letzte 12 Stunden',
  '24h': 'Letzte 24 Stunden',
  '7d': 'Letzte 7 Tage',
  '30d': 'Letzte 30 Tage',
  '90d': 'Letzte 90 Tage',
  '365d': 'Letztes Jahr',
  'all': 'Unbegrenzt',
};

/// The owner's dashboard: what is playing (with stop), bandwidth / processor / memory / streams over time,
/// users, and the play history. Admin only (the server refuses everyone else).
class DashboardScreen extends StatefulWidget {
  final Api api;
  const DashboardScreen({super.key, required this.api});
  @override
  State<DashboardScreen> createState() => _DashboardScreenState();
}

class _DashboardScreenState extends State<DashboardScreen> {
  Map<String, dynamic>? d;
  Map<String, dynamic>? series;
  Map<String, dynamic>? hist;
  String range = 'realtime';
  int days = 30;
  bool hours = false;
  String user = '';
  String? error;
  Timer? timer;
  DateTime seriesAt = DateTime.fromMillisecondsSinceEpoch(0);

  Api get api => widget.api;

  @override
  void initState() {
    super.initState();
    live();
    loadHistory();
  }

  @override
  void dispose() {
    timer?.cancel();
    super.dispose();
  }

  Future<void> live() async {
    timer?.cancel();
    try {
      final r = Map<String, dynamic>.from(await api.get('/api/dashboard'));
      if (!mounted) return;
      setState(() {
        d = r;
        error = null;
      });
      if (range == 'realtime' || series == null || DateTime.now().difference(seriesAt).inSeconds > 60) await loadSeries();
      timer = Timer(Duration(seconds: (r['sample_seconds'] ?? 5) as int), live);
    } catch (e) {
      if (!mounted) return;
      setState(() => error = '$e');
      timer = Timer(const Duration(seconds: 10), live);
    }
  }

  Future<void> loadSeries() async {
    try {
      final r = Map<String, dynamic>.from(await api.get('/api/dashboard/stats?range=$range'));
      seriesAt = DateTime.now();
      if (mounted) setState(() => series = r);
    } catch (_) {}
  }

  Future<void> loadHistory() async {
    try {
      final r = Map<String, dynamic>.from(await api.get('/api/dashboard/history?days=$days${user.isEmpty ? '' : '&user=$user'}'));
      if (mounted) setState(() => hist = r);
    } catch (_) {}
  }

  Future<void> stop(Map p, String title) async {
    final msg = TextEditingController();
    final ok = await showDialog<bool>(
      context: context,
      builder: (c) => AlertDialog(
        title: const Text('Wiedergabe beenden?'),
        content: Column(mainAxisSize: MainAxisSize.min, crossAxisAlignment: CrossAxisAlignment.start, children: [
          Text('„$title“ endet sofort für ${p['user'] ?? '?'}.'),
          const SizedBox(height: 12),
          TextField(controller: msg, maxLines: 3, maxLength: 500, decoration: InputDecoration(border: const OutlineInputBorder(), hintText: 'Nachricht an ${p['user'] ?? '?'} (optional)')),
        ]),
        actions: [
          TextButton(onPressed: () => Navigator.pop(c, false), child: const Text('Abbrechen')),
          FilledButton(onPressed: () => Navigator.pop(c, true), child: const Text('Wiedergabe beenden')),
        ],
      ),
    );
    if (ok != true) return;
    try {
      await api.post('/api/dashboard/stop', {'user': p['user_id'], 'item': p['id'], 'message': msg.text});
      if (mounted) ScaffoldMessenger.of(context).showSnackBar(const SnackBar(content: Text('Wiedergabe beendet')));
      live();
    } catch (e) {
      if (mounted) ScaffoldMessenger.of(context).showSnackBar(SnackBar(content: Text('$e')));
    }
  }

  @override
  Widget build(BuildContext context) {
    final data = d;
    return Scaffold(
      appBar: AppBar(title: const Text('Dashboard')),
      body: data == null
          ? Center(child: error != null ? Text(error!) : const CircularProgressIndicator())
          : ListView(padding: const EdgeInsets.all(20), children: [
              Text(
                'Läuft seit ${fmtUptime((data['now'] ?? 0) - (data['started_at'] ?? 0))} · Version ${data['version']} · ${fmtBytes(data['bytes_sent'] ?? 0)} gesendet · aktualisiert alle ${data['sample_seconds']} s',
                style: const TextStyle(color: Colors.white60),
              ),
              const SizedBox(height: 16),
              _tiles(data),
              _h('Läuft gerade'),
              if ((data['playing'] as List).isEmpty)
                const Padding(padding: EdgeInsets.all(22), child: Center(child: Text('Gerade läuft nichts.', style: TextStyle(color: Colors.white54))))
              else
                for (final p in data['playing'] as List) _playing(Map<String, dynamic>.from(p)),
              _h('Nutzer'),
              _users(data),
              Row(children: [
                Expanded(child: _h('Aktivität')),
                DropdownButton<String>(
                  value: range,
                  items: [for (final e in _ranges.entries) DropdownMenuItem(value: e.key, child: Text(e.value))],
                  onChanged: (v) async {
                    if (v == null) return;
                    setState(() => range = v);
                    await loadSeries();
                  },
                ),
              ]),
              ..._charts(data),
              _history(data),
            ]),
    );
  }

  Widget _h(String t) => Padding(padding: const EdgeInsets.only(top: 22, bottom: 8), child: Text(t, style: Theme.of(context).textTheme.titleMedium));

  Widget _tile(String label, String value, String note) => Container(
        width: 210,
        padding: const EdgeInsets.all(14),
        decoration: BoxDecoration(color: Colors.white10, borderRadius: BorderRadius.circular(10)),
        child: Column(crossAxisAlignment: CrossAxisAlignment.start, children: [
          Text(label, style: const TextStyle(color: Colors.white60, fontSize: 12)),
          const SizedBox(height: 4),
          Text(value, style: const TextStyle(fontSize: 26, fontWeight: FontWeight.w600)),
          const SizedBox(height: 2),
          Text(note, style: const TextStyle(color: Colors.white54, fontSize: 12)),
        ]),
      );

  Widget _tiles(Map<String, dynamic> data) {
    final c = Map<String, dynamic>.from(data['current'] ?? const {});
    final playing = data['playing'] as List;
    final users = data['users'] as List;
    final tc = playing.where((p) => p['method'] == 'transcode').length;
    final active = users.where((u) => u['active'] == true).length;
    final out = (c['out_bps'] ?? 0) as num;
    final outRemote = (c['out_remote_bps'] ?? 0) as num;
    final memTotal = (c['mem_total'] ?? 0) as num;
    return Wrap(spacing: 12, runSpacing: 12, children: [
      _tile('Läuft gerade', '${playing.length}', tc > 0 ? '$tc davon umgewandelt${(data['max_transcodes'] ?? 0) > 0 ? ' (höchstens ${data['max_transcodes']} gleichzeitig)' : ''}' : 'keine umgewandelt'),
      _tile('Bandbreite', fmtBits(out), out > 0 ? '${fmtBits((out - outRemote).clamp(0, double.infinity))} lokal · ${fmtBits(outRemote)} extern' : 'wird gerade an Player gesendet'),
      _tile('Prozessor', fmtPct(c['cpu']), c['cpu_server'] != null ? 'Lichtspiel: ${fmtPct(c['cpu_server'])} · ${data['cores']} Kerne' : '${data['cores']} Kerne'),
      _tile('Arbeitsspeicher', memTotal > 0 ? fmtPct(100 * (c['mem_used'] as num) / memTotal) : '–', memTotal > 0 ? '${fmtBytes(c['mem_used'])} / ${fmtBytes(memTotal)}${c['mem_server'] != null ? ' · Lichtspiel: ${fmtBytes(c['mem_server'])}' : ''}' : ''),
      _tile('Aktive Nutzer', '$active', 'von ${users.length} · aktiv = gesehen in den letzten ${((data['active_seconds'] ?? 0) / 60).round()} min'),
    ]);
  }

  Widget _playing(Map<String, dynamic> p) {
    final pct = ((p['duration'] ?? 0) as num) > 0 ? ((p['offset'] ?? 0) / p['duration']).clamp(0, 1).toDouble() : 0.0;
    final title = p['type'] == 'episode' && p['grandparentTitle'] != null ? '${p['grandparentTitle']}' : '${p['title'] ?? '(entfernt)'}';
    final sub = p['type'] == 'episode' ? 'S${p['parentIndex'] ?? 0} E${p['index'] ?? 0} · ${p['title']}' : '${p['year'] ?? ''}';
    final method = switch (p['method']) { 'transcode' => 'Umgewandelt', 'remux' => 'Umgepackt', _ => 'Direkt' };
    final rows = <List<String>>[
      ['Gerät', '${p['client'] ?? '–'}'],
      ['Verbindung', '${p['local'] == true ? 'Lokal' : 'Extern'}${p['ip'] != null ? ' (${p['ip']})' : ''} — ${fmtBits(p['out_bps'] ?? 0)}'],
      if (p['video'] is Map) ['Video', _videoLine(p['video'])],
      if (p['audio'] is Map) ['Ton', _audioLine(p['audio'])],
    ];
    return Card(
      margin: const EdgeInsets.only(bottom: 10),
      child: Padding(
        padding: const EdgeInsets.all(12),
        child: Row(crossAxisAlignment: CrossAxisAlignment.start, children: [
          InkWell(
            onTap: () => openItem(context, api, {'ratingKey': p['id']}),
            child: SizedBox(width: 70, child: AspectRatio(aspectRatio: 2 / 3, child: Image.network(api.img(p['thumb']), fit: BoxFit.cover, errorBuilder: (_, __, ___) => Container(color: Colors.white10)))),
          ),
          const SizedBox(width: 14),
          Expanded(
            child: Column(crossAxisAlignment: CrossAxisAlignment.start, children: [
              Row(children: [
                Expanded(child: Column(crossAxisAlignment: CrossAxisAlignment.start, children: [
                  Text(title, style: const TextStyle(fontWeight: FontWeight.w600)),
                  Text(sub, style: const TextStyle(color: Colors.white60)),
                ])),
                IconButton(tooltip: 'Wiedergabe beenden', icon: const Icon(Icons.stop_circle_outlined), onPressed: () => stop(p, p['type'] == 'episode' ? '$title · ${p['title']}' : title)),
              ]),
              const SizedBox(height: 4),
              Row(children: [
                Text('${p['user'] ?? '?'}', style: const TextStyle(fontWeight: FontWeight.bold)),
                const SizedBox(width: 8),
                Chip(visualDensity: VisualDensity.compact, label: Text(method), backgroundColor: p['method'] == 'transcode' ? Colors.orange.withValues(alpha: .25) : null),
              ]),
              const SizedBox(height: 4),
              Table(columnWidths: const {0: IntrinsicColumnWidth()}, children: [
                for (final r in rows)
                  TableRow(children: [
                    Padding(padding: const EdgeInsets.only(right: 14, bottom: 2), child: Text(r[0], style: const TextStyle(color: Colors.white54, fontSize: 13))),
                    Padding(padding: const EdgeInsets.only(bottom: 2), child: Text(r[1], style: const TextStyle(fontSize: 13))),
                  ]),
              ]),
              const SizedBox(height: 8),
              Row(children: [
                Text(p['state'] == 'paused' ? '⏸ Pausiert' : '▶ Läuft', style: const TextStyle(fontSize: 12)),
                const SizedBox(width: 10),
                Expanded(child: LinearProgressIndicator(value: pct, minHeight: 4)),
                const SizedBox(width: 10),
                Text(((p['duration'] ?? 0) as num) > 0 ? '${fmtClockMs(p['offset'] ?? 0)} / ${fmtClockMs(p['duration'])}' : '', style: const TextStyle(fontSize: 12, color: Colors.white60)),
              ]),
            ]),
          ),
        ]),
      ),
    );
  }

  Widget _users(Map<String, dynamic> data) {
    final now = data['now'] ?? 0;
    final users = (data['users'] as List).cast<Map>();
    return SingleChildScrollView(
      scrollDirection: Axis.horizontal,
      child: DataTable(
        columns: const [
          DataColumn(label: Text('Nutzer')),
          DataColumn(label: Text('Status')),
          DataColumn(label: Text('Zuletzt gesehen')),
          DataColumn(label: Text('Gerät')),
          DataColumn(label: Text('Adresse')),
          DataColumn(label: Text('Spielt')),
          DataColumn(label: Text('Bandbreite')),
          DataColumn(label: Text('Letzte Wiedergabe')),
        ],
        rows: [
          for (final u in users)
            DataRow(cells: [
              DataCell(Text('${u['username']}${u['admin'] == true ? ' (Admin)' : ''}', style: const TextStyle(fontWeight: FontWeight.bold))),
              DataCell(Row(children: [
                Icon(Icons.circle, size: 10, color: u['active'] == true ? Colors.greenAccent : Colors.grey),
                const SizedBox(width: 6),
                Text(u['active'] == true ? 'aktiv' : 'abwesend'),
              ])),
              DataCell(Text(u['last_seen'] != null ? fmtAgo(u['last_seen'], now) : 'seit dem Start nicht')),
              DataCell(Text('${u['client'] ?? '–'}')),
              DataCell(Text('${u['ip'] ?? '–'}')),
              DataCell(Text(u['playing'] != null ? '${u['playing']}' : '–')),
              DataCell(Text(((u['out_bps'] ?? 0) as num) > 0 ? fmtBits(u['out_bps']) : '–')),
              DataCell(Text(u['last_play'] != null ? fmtDateTime(u['last_play']) : '–')),
            ]),
        ],
      ),
    );
  }

  List<Widget> _charts(Map<String, dynamic> data) {
    final r = series;
    if (r == null) return [const Padding(padding: EdgeInsets.all(20), child: Center(child: CircularProgressIndicator()))];
    final ss = ((r['samples'] as List?) ?? const []).cast<Map>();
    final c = Map<String, dynamic>.from(data['current'] ?? const {});
    num? v(Map s, String k) => s[k] as num?;
    final peak = ss.fold<num>(0, (m, s) => ((s['out_bps'] ?? 0) as num) * 8 > m ? ((s['out_bps'] ?? 0) as num) * 8 : m);
    final (div, unit) = peak >= 1e9 ? (1e9, 'Gbit/s') : peak >= 1e6 ? (1e6, 'Mbit/s') : peak >= 1e3 ? (1e3, 'kbit/s') : (1.0, 'bit/s');
    final memMax = ((c['mem_total'] ?? 0) as num) > 0 ? (c['mem_total'] as num) / 1e9 : null;
    final times = [for (final s in ss) (s['at'] as num)];
    return [
      _chart('Bandbreite · $unit', times, [
        _Series('Lokal', Colors.lightBlueAccent, [for (final s in ss) s['out_bps'] == null ? null : (((s['out_bps'] as num) - ((s['out_remote_bps'] ?? 0) as num)).clamp(0, double.infinity)) * 8 / div]),
        _Series('Extern', Colors.orangeAccent, [for (final s in ss) s['out_bps'] == null ? null : ((s['out_remote_bps'] ?? 0) as num) * 8 / div]),
      ], null, unit: unit),
      _chart('Prozessor', times, [
        _Series('System', Colors.lightBlueAccent, [for (final s in ss) v(s, 'cpu')]),
        _Series('Lichtspiel', Colors.orangeAccent, [for (final s in ss) v(s, 'cpu_server')]),
      ], 100, unit: '%'),
      _chart('Arbeitsspeicher', times, [
        _Series('System', Colors.lightBlueAccent, [for (final s in ss) v(s, 'mem_used') == null ? null : v(s, 'mem_used')! / 1e9]),
        _Series('Lichtspiel', Colors.orangeAccent, [for (final s in ss) v(s, 'mem_server') == null ? null : v(s, 'mem_server')! / 1e9]),
      ], memMax, unit: 'GB'),
      _chart('Streams', times, [
        _Series('Läuft', Colors.lightBlueAccent, [for (final s in ss) v(s, 'streams')]),
        _Series('Umgewandelt', Colors.orangeAccent, [for (final s in ss) v(s, 'transcodes')]),
      ], ss.fold<num>(4, (m, s) => ((s['streams'] ?? 0) as num) > m ? (s['streams'] as num) : m), unit: ''),
    ];
  }

  Widget _chart(String title, List<num> times, List<_Series> series, num? max, {required String unit}) => Card(
        margin: const EdgeInsets.only(bottom: 12),
        child: Padding(
          padding: const EdgeInsets.all(12),
          child: Column(crossAxisAlignment: CrossAxisAlignment.start, children: [
            Row(children: [
              Text(title, style: const TextStyle(fontWeight: FontWeight.w600)),
              const Spacer(),
              for (final s in series) ...[Icon(Icons.circle, size: 10, color: s.color), const SizedBox(width: 4), Text(s.name, style: const TextStyle(fontSize: 12)), const SizedBox(width: 12)],
            ]),
            const SizedBox(height: 8),
            SizedBox(height: 150, width: double.infinity, child: CustomPaint(painter: _LinePainter(times, series, max, unit))),
          ]),
        ),
      );

  Widget _history(Map<String, dynamic> data) {
    final r = hist;
    final users = (data['users'] as List).cast<Map>();
    final head = Row(children: [
      Expanded(child: _h('Wiedergabeverlauf')),
      SegmentedButton<int>(
        showSelectedIcon: false,
        segments: const [ButtonSegment(value: 7, label: Text('7 T')), ButtonSegment(value: 30, label: Text('30 T')), ButtonSegment(value: 90, label: Text('90 T')), ButtonSegment(value: 365, label: Text('1 J'))],
        selected: {days},
        onSelectionChanged: (s) {
          days = s.first;
          loadHistory();
          setState(() {});
        },
      ),
      const SizedBox(width: 8),
      SegmentedButton<bool>(
        showSelectedIcon: false,
        segments: const [ButtonSegment(value: false, label: Text('Wiedergaben')), ButtonSegment(value: true, label: Text('Stunden'))],
        selected: {hours},
        onSelectionChanged: (s) => setState(() => hours = s.first),
      ),
      const SizedBox(width: 8),
      DropdownButton<String>(
        value: user,
        items: [const DropdownMenuItem(value: '', child: Text('Alle')), for (final u in users) DropdownMenuItem(value: '${u['id']}', child: Text('${u['username']}'))],
        onChanged: (v) {
          user = v ?? '';
          loadHistory();
          setState(() {});
        },
      ),
    ]);
    if (r == null) return Column(crossAxisAlignment: CrossAxisAlignment.start, children: [head, const Center(child: CircularProgressIndicator())]);
    final weekly = days > 90;
    final today = DateTime.now();
    final day0 = DateTime(today.year, today.month, today.day);
    var start = day0.subtract(Duration(days: days - 1));
    if (weekly) start = start.subtract(Duration(days: (start.weekday - 1) % 7));
    final step = Duration(days: weekly ? 7 : 1);
    final buckets = <_Bucket>[];
    for (var t = start; !t.isAfter(day0); t = t.add(step)) {
      buckets.add(_Bucket(t.millisecondsSinceEpoch / 1000, t.add(step).millisecondsSinceEpoch / 1000, '${t.day}.${t.month}.'));
    }
    for (final p in (r['plays'] as List)) {
      final at = p[0] as num;
      final kind = '${p[2]}';
      final mins = (p[3] ?? 0) as num;
      final b = buckets.where((x) => at >= x.from && at < x.to).firstOrNull;
      if (b == null) continue;
      b.parts[kind] = (b.parts[kind] ?? 0) + (hours ? mins / 60 : 1);
    }
    String what(Map it) => it['type'] == 'episode' && it['grandparentTitle'] != null
        ? '${it['grandparentTitle']} · S${it['parentIndex'] ?? 0}E${it['index'] ?? 0} ${it['title']}'
        : '${it['title'] ?? '(entfernt)'}';
    String dur(num mins) => mins >= 60 ? '${(mins / 60).floor()} h ${(mins % 60).round()} min' : '${mins.round()} min';
    final titles = (r['titles'] as List).cast<Map>();
    final topUsers = (r['users'] as List).cast<Map>();
    final recent = (r['recent'] as List).cast<Map>();
    return Column(crossAxisAlignment: CrossAxisAlignment.start, children: [
      head,
      Card(
        child: Padding(
          padding: const EdgeInsets.all(12),
          child: Column(crossAxisAlignment: CrossAxisAlignment.start, children: [
            Row(children: [
              Text('${hours ? 'Gesehene Stunden' : 'Wiedergaben'} pro ${weekly ? 'Woche' : 'Tag'}', style: const TextStyle(fontWeight: FontWeight.w600)),
              const Spacer(),
              Text('${r['size']} Wiedergaben · ${dur(r['minutes'] ?? 0)} gesehen', style: const TextStyle(color: Colors.white60, fontSize: 12)),
            ]),
            const SizedBox(height: 8),
            SizedBox(height: 180, width: double.infinity, child: CustomPaint(painter: _BarPainter(buckets))),
            const SizedBox(height: 6),
            const Row(children: [
              Icon(Icons.circle, size: 10, color: Colors.lightBlueAccent), SizedBox(width: 4), Text('Filme', style: TextStyle(fontSize: 12)), SizedBox(width: 12),
              Icon(Icons.circle, size: 10, color: Colors.orangeAccent), SizedBox(width: 4), Text('Folgen', style: TextStyle(fontSize: 12)), SizedBox(width: 12),
              Icon(Icons.circle, size: 10, color: Colors.grey), SizedBox(width: 4), Text('Sonstiges', style: TextStyle(fontSize: 12)),
            ]),
          ]),
        ),
      ),
      const SizedBox(height: 12),
      Wrap(spacing: 12, runSpacing: 12, children: [
        SizedBox(
          width: 420,
          child: Card(
            child: Padding(
              padding: const EdgeInsets.all(12),
              child: Column(crossAxisAlignment: CrossAxisAlignment.start, children: [
                const Text('Am häufigsten gesehen', style: TextStyle(fontWeight: FontWeight.w600)),
                if (titles.isEmpty) const Padding(padding: EdgeInsets.all(8), child: Text('Keine Wiedergaben in dieser Zeit.', style: TextStyle(color: Colors.white54))),
                for (final x in titles)
                  ListTile(
                    dense: true,
                    contentPadding: EdgeInsets.zero,
                    leading: SizedBox(width: 34, child: Image.network(api.img(x['thumb']), fit: BoxFit.cover, errorBuilder: (_, __, ___) => Container(color: Colors.white10))),
                    title: Text('${x['title'] ?? '(entfernt)'}', maxLines: 1, overflow: TextOverflow.ellipsis),
                    subtitle: Text([x['year'], if (x['type'] == 'show') 'Serie'].where((e) => e != null).join(' · ')),
                    trailing: Text('${x['plays']}×'),
                    onTap: x['id'] == null ? null : () => openItem(context, api, {'ratingKey': x['id']}),
                  ),
              ]),
            ),
          ),
        ),
        SizedBox(
          width: 420,
          child: Card(
            child: Padding(
              padding: const EdgeInsets.all(12),
              child: Column(crossAxisAlignment: CrossAxisAlignment.start, children: [
                const Text('Top-Nutzer', style: TextStyle(fontWeight: FontWeight.w600)),
                if (topUsers.isEmpty) const Padding(padding: EdgeInsets.all(8), child: Text('Keine Wiedergaben in dieser Zeit.', style: TextStyle(color: Colors.white54))),
                for (final x in topUsers)
                  ListTile(dense: true, contentPadding: EdgeInsets.zero, title: Text('${x['username'] ?? '?'}'), trailing: Text('${x['plays']} Wiedergaben · ${dur(x['minutes'] ?? 0)}', style: const TextStyle(fontSize: 12, color: Colors.white60))),
              ]),
            ),
          ),
        ),
      ]),
      const SizedBox(height: 12),
      Card(
        child: Padding(
          padding: const EdgeInsets.all(12),
          child: Column(crossAxisAlignment: CrossAxisAlignment.start, children: [
            const Text('Letzte Wiedergaben', style: TextStyle(fontWeight: FontWeight.w600)),
            if (recent.isEmpty) const Padding(padding: EdgeInsets.all(8), child: Text('Keine Wiedergaben in dieser Zeit.', style: TextStyle(color: Colors.white54))),
            for (final e in recent)
              ListTile(
                dense: true,
                contentPadding: EdgeInsets.zero,
                title: Text(what(Map.from(e['item'] ?? const {})), maxLines: 1, overflow: TextOverflow.ellipsis),
                subtitle: Text('${fmtDateTime(e['at'])} · ${e['user'] ?? '?'}'),
                trailing: Text(e['manual'] == true ? 'von Hand markiert' : '${dur(e['minutes'] ?? 0)}${e['live'] == true ? ' · läuft gerade' : ''}', style: const TextStyle(fontSize: 12, color: Colors.white60)),
                onTap: (e['item'] is Map && e['item']['id'] != null) ? () => openItem(context, api, {'ratingKey': e['item']['id']}) : null,
              ),
          ]),
        ),
      ),
    ]);
  }
}

class _Series {
  final String name;
  final Color color;
  final List<num?> values;
  _Series(this.name, this.color, this.values);
}

class _LinePainter extends CustomPainter {
  final List<num> times;
  final List<_Series> series;
  final num? max;
  final String unit;
  _LinePainter(this.times, this.series, this.max, this.unit);

  @override
  void paint(Canvas canvas, Size size) {
    const left = 44.0, bottom = 18.0;
    final w = size.width - left, h = size.height - bottom;
    var top = max?.toDouble() ?? 0;
    if (max == null) {
      for (final s in series) {
        for (final v in s.values) {
          if (v != null && v > top) top = v.toDouble();
        }
      }
    }
    if (top <= 0) top = 1;
    final grid = Paint()..color = Colors.white12..strokeWidth = 1;
    for (var i = 0; i <= 4; i++) {
      final y = h - h * i / 4;
      canvas.drawLine(Offset(left, y), Offset(size.width, y), grid);
      final v = top * i / 4;
      _text(canvas, v >= 10 ? v.toStringAsFixed(0) : v.toStringAsFixed(1), Offset(0, y - 6), 11);
    }
    if (times.length < 2) return;
    final t0 = times.first.toDouble(), t1 = times.last.toDouble();
    final span = (t1 - t0) <= 0 ? 1 : t1 - t0;
    double x(num t) => left + w * (t - t0) / span;
    for (final s in series) {
      final path = Path();
      var pen = false;
      for (var i = 0; i < times.length; i++) {
        final v = s.values[i];
        if (v == null) {
          pen = false;
          continue;
        }
        final p = Offset(x(times[i]), h - h * (v.toDouble() / top).clamp(0, 1));
        if (pen) {
          path.lineTo(p.dx, p.dy);
        } else {
          path.moveTo(p.dx, p.dy);
          pen = true;
        }
      }
      canvas.drawPath(path, Paint()..color = s.color..style = PaintingStyle.stroke..strokeWidth = 2);
    }
    final long = (t1 - t0) > 36 * 3600;
    for (final f in [0.0, 0.5, 1.0]) {
      final t = DateTime.fromMillisecondsSinceEpoch(((t0 + span * f) * 1000).round());
      String p(int n) => n.toString().padLeft(2, '0');
      _text(canvas, long ? '${t.day}.${t.month}.' : '${p(t.hour)}:${p(t.minute)}', Offset((left + w * f - (f == 1 ? 30 : f == 0.5 ? 14 : 0)), h + 3), 11);
    }
  }

  void _text(Canvas c, String s, Offset o, double size) {
    final tp = TextPainter(text: TextSpan(text: s, style: TextStyle(color: Colors.white54, fontSize: size)), textDirection: TextDirection.ltr)..layout();
    tp.paint(c, o);
  }

  @override
  bool shouldRepaint(_LinePainter old) => true;
}

class _Bucket {
  final double from, to;
  final String label;
  final Map<String, num> parts = {};
  _Bucket(this.from, this.to, this.label);
}

class _BarPainter extends CustomPainter {
  final List<_Bucket> buckets;
  _BarPainter(this.buckets);

  @override
  void paint(Canvas canvas, Size size) {
    const left = 36.0, bottom = 18.0;
    final w = size.width - left, h = size.height - bottom;
    double total(_Bucket b) => b.parts.values.fold<double>(0, (a, v) => a + v.toDouble());
    var top = buckets.fold<double>(0, (m, b) => total(b) > m ? total(b) : m);
    if (top <= 0) top = 1;
    final grid = Paint()..color = Colors.white12;
    for (var i = 0; i <= 4; i++) {
      final y = h - h * i / 4;
      canvas.drawLine(Offset(left, y), Offset(size.width, y), grid);
      final tp = TextPainter(text: TextSpan(text: (top * i / 4).toStringAsFixed(top < 10 ? 1 : 0), style: const TextStyle(color: Colors.white54, fontSize: 11)), textDirection: TextDirection.ltr)..layout();
      tp.paint(canvas, Offset(0, y - 6));
    }
    if (buckets.isEmpty) return;
    final bw = w / buckets.length;
    const colors = {'movie': Colors.lightBlueAccent, 'episode': Colors.orangeAccent};
    for (var i = 0; i < buckets.length; i++) {
      var y = h;
      for (final k in ['movie', 'episode', 'other']) {
        final v = buckets[i].parts[k] ?? 0;
        if (v <= 0) continue;
        final bh = h * v.toDouble() / top;
        canvas.drawRect(Rect.fromLTWH(left + bw * i + bw * .12, y - bh, bw * .76, bh), Paint()..color = colors[k] ?? Colors.grey);
        y -= bh;
      }
    }
    final step = (buckets.length / 6).ceil().clamp(1, 1000);
    for (var i = 0; i < buckets.length; i += step) {
      final tp = TextPainter(text: TextSpan(text: buckets[i].label, style: const TextStyle(color: Colors.white54, fontSize: 11)), textDirection: TextDirection.ltr)..layout();
      tp.paint(canvas, Offset(left + bw * i, h + 3));
    }
  }

  @override
  bool shouldRepaint(_BarPainter old) => true;
}
