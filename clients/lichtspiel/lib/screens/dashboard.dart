import 'dart:async';
import 'package:flutter/material.dart';
import '../api.dart';
import '../clock.dart';
import 'home.dart';
import '../theme.dart';
import '../i18n.dart';

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

String fmtUptime(num sec) {
  final d = sec ~/ 86400, h = (sec % 86400) ~/ 3600, m = (sec % 3600) ~/ 60;
  return d > 0 ? '$d d $h h' : h > 0 ? '$h h $m min' : '$m min';
}

String fmtAgo(num at, num now) {
  final d = now - at;
  if (d < 90) return tr('just now');
  if (d < 5400) return tr('{x} min ago', {'x': (d / 60).round()});
  if (d < 129600) return tr('{x} h ago', {'x': (d / 3600).round()});
  return tr('{x} days ago', {'x': (d / 86400).round()});
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
  final from = '${_res(v['height'])} (${[(_codec(v['codec'])), if (v['profile'] != null) '${v['profile']}', if (v['hdr'] == true) 'HDR'].join(' ')})';
  return v['action'] == 'transcode' ? tr('{from} → {res} ({codec}) — converted{v}', {'from': from, 'res': _res(v['out_height']), 'codec': _codec(v['out_codec']), 'v': v['hw'] == true ? tr(' (hardware)') : ''}) : tr('{from} — direct', {'from': from});
}

String _audioLine(Map a) {
  final lang = a['language'] ?? a['title'] ?? tr('Unknown');
  final from = '$lang (${[_codec(a['codec']), _ch(a['channels'])].where((s) => s.isNotEmpty).join(' ')})';
  return a['action'] == 'transcode' ? tr('{from} → {codec} {ch} — converted', {'from': from, 'codec': _codec(a['out_codec']), 'ch': _ch(a['out_channels'])}) : tr('{from} — direct', {'from': from});
}

Map<String, String> get _ranges => {
  'realtime': tr('Real time'),
  '12h': tr('Last 12 hours'),
  '24h': tr('Last 24 hours'),
  '7d': tr('Last 7 days'),
  '30d': tr('Last 30 days'),
  '90d': tr('Last 90 days'),
  '365d': tr('Last year'),
  'all': tr('All time'),
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
        title: Text(tr('Stop playback?')),
        content: Column(mainAxisSize: MainAxisSize.min, crossAxisAlignment: CrossAxisAlignment.start, children: [
          Text(tr('“{title}” stops at once for {p}.', {'title': title, 'p': p['user'] ?? '?'})),
          const SizedBox(height: 12),
          TextField(controller: msg, maxLines: 3, maxLength: 500, decoration: InputDecoration(border: const OutlineInputBorder(), hintText: tr('Message to {p} (optional)', {'p': p['user'] ?? '?'}))),
        ]),
        actions: [
          TextButton(onPressed: () => Navigator.pop(c, false), child: Text(tr('Cancel'))),
          FilledButton(onPressed: () => Navigator.pop(c, true), child: Text(tr('Stop playback'))),
        ],
      ),
    );
    if (ok != true) return;
    try {
      await api.post('/api/dashboard/stop', {'user': p['user_id'], 'item': p['id'], 'message': msg.text});
      if (mounted) ScaffoldMessenger.of(context).showSnackBar(SnackBar(content: Text(tr('Playback stopped'))));
      live();
    } catch (e) {
      if (mounted) ScaffoldMessenger.of(context).showSnackBar(SnackBar(content: Text('$e')));
    }
  }

  @override
  Widget build(BuildContext context) {
    final data = d;
    return Scaffold(
      appBar: AppBar(title: Text(tr('Dashboard'))),
      body: data == null
          ? Center(child: error != null ? Text(error!) : const CircularProgressIndicator())
          : ListView(padding: const EdgeInsets.all(20), children: [
              Text(
                tr('Running for {fmtUptime} · version {data} · {fmtBytes} sent · updated every {data2} s', {'fmtUptime': fmtUptime((data['now'] ?? 0) - (data['started_at'] ?? 0)), 'data': data['version'], 'fmtBytes': fmtBytes(data['bytes_sent'] ?? 0), 'data2': data['sample_seconds']}),
                style: TextStyle(color: context.dim),
              ),
              const SizedBox(height: 16),
              _tiles(data),
              _h(tr('Playing now')),
              if ((data['playing'] as List).isEmpty)
                Padding(padding: EdgeInsets.all(22), child: Center(child: Text(tr('Nothing is playing.'), style: TextStyle(color: context.faint))))
              else
                for (final p in data['playing'] as List) _playing(Map<String, dynamic>.from(p)),
              _h(tr('Users')),
              _users(data),
              Row(children: [
                Expanded(child: _h(tr('Activity'))),
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
        decoration: BoxDecoration(color: context.hair, borderRadius: BorderRadius.circular(10)),
        child: Column(crossAxisAlignment: CrossAxisAlignment.start, children: [
          Text(label, style: TextStyle(color: context.dim, fontSize: 12)),
          const SizedBox(height: 4),
          Text(value, style: const TextStyle(fontSize: 26, fontWeight: FontWeight.w600)),
          const SizedBox(height: 2),
          Text(note, style: TextStyle(color: context.faint, fontSize: 12)),
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
      _tile(tr('Playing now'), '${playing.length}', tc > 0 ? tr('{tc} of them converted{x}', {'tc': tc, 'x': (data['max_transcodes'] ?? 0) > 0 ? tr(' (at most {data} at once)', {'data': data['max_transcodes']}) : ''}) : tr('none converted')),
      _tile(tr('Bandwidth'), fmtBits(out), out > 0 ? tr('{fmtBits} local · {fmtBits2} remote', {'fmtBits': fmtBits((out - outRemote).clamp(0, double.infinity)), 'fmtBits2': fmtBits(outRemote)}) : tr('being sent to players')),
      _tile(tr('Processor'), fmtPct(c['cpu']), c['cpu_server'] != null ? tr('Lichtspiel: {fmtPct} · {data} cores', {'fmtPct': fmtPct(c['cpu_server']), 'data': data['cores']}) : tr('{data} cores', {'data': data['cores']})),
      _tile(tr('Memory'), memTotal > 0 ? fmtPct(100 * (c['mem_used'] as num) / memTotal) : '–', memTotal > 0 ? '${fmtBytes(c['mem_used'])} / ${fmtBytes(memTotal)}${c['mem_server'] != null ? tr(' · Lichtspiel: {fmtBytes}', {'fmtBytes': fmtBytes(c['mem_server'])}) : ''}' : ''),
      _tile(tr('Active users'), '$active', tr('of {length} · active = seen in the last {x} min', {'length': users.length, 'x': ((data['active_seconds'] ?? 0) / 60).round()})),
    ]);
  }

  Widget _playing(Map<String, dynamic> p) {
    final pct = ((p['duration'] ?? 0) as num) > 0 ? ((p['offset'] ?? 0) / p['duration']).clamp(0, 1).toDouble() : 0.0;
    final title = p['type'] == 'episode' && p['grandparentTitle'] != null ? '${p['grandparentTitle']}' : '${p['title'] ?? tr('(removed)')}';
    final sub = p['type'] == 'episode' ? 'S${p['parentIndex'] ?? 0} E${p['index'] ?? 0} · ${p['title']}' : '${p['year'] ?? ''}';
    final method = switch (p['method']) { 'transcode' => tr('Converted'), 'remux' => tr('Remuxed'), _ => tr('Direct') };
    final rows = <List<String>>[
      [tr('Device'), '${p['client'] ?? '–'}'],
      [tr('Connection'), '${p['local'] == true ? tr('Local') : tr('Remote')}${p['ip'] != null ? ' (${p['ip']})' : ''} — ${fmtBits(p['out_bps'] ?? 0)}'],
      if (p['video'] is Map) [tr('Video'), _videoLine(p['video'])],
      if (p['audio'] is Map) [tr('Sound'), _audioLine(p['audio'])],
    ];
    return Card(
      margin: const EdgeInsets.only(bottom: 10),
      child: Padding(
        padding: const EdgeInsets.all(12),
        child: Row(crossAxisAlignment: CrossAxisAlignment.start, children: [
          InkWell(
            onTap: () => openItem(context, api, {'ratingKey': p['id']}),
            child: SizedBox(width: 70, child: AspectRatio(aspectRatio: 2 / 3, child: Image.network(api.img(p['thumb']), headers: api.auth, fit: BoxFit.cover, cacheWidth: 160, errorBuilder: (_, _, _) => Container(color: context.hair)))),
          ),
          const SizedBox(width: 14),
          Expanded(
            child: Column(crossAxisAlignment: CrossAxisAlignment.start, children: [
              Row(children: [
                Expanded(child: Column(crossAxisAlignment: CrossAxisAlignment.start, children: [
                  Text(title, style: const TextStyle(fontWeight: FontWeight.w600)),
                  Text(sub, style: TextStyle(color: context.dim)),
                ])),
                IconButton(tooltip: tr('Stop playback'), icon: const Icon(Icons.stop_circle_outlined), onPressed: () => stop(p, p['type'] == 'episode' ? '$title · ${p['title']}' : title)),
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
                    Padding(padding: const EdgeInsets.only(right: 14, bottom: 2), child: Text(r[0], style: TextStyle(color: context.faint, fontSize: 13))),
                    Padding(padding: const EdgeInsets.only(bottom: 2), child: Text(r[1], style: const TextStyle(fontSize: 13))),
                  ]),
              ]),
              const SizedBox(height: 8),
              Row(children: [
                Text(p['state'] == 'paused' ? tr('⏸ Paused') : tr('▶ Playing'), style: const TextStyle(fontSize: 12)),
                const SizedBox(width: 10),
                Expanded(child: LinearProgressIndicator(value: pct, minHeight: 4)),
                const SizedBox(width: 10),
                Text(((p['duration'] ?? 0) as num) > 0 ? '${fmtClock(p['offset'] ?? 0)} / ${fmtClock(p['duration'])}' : '', style: TextStyle(fontSize: 12, color: context.dim)),
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
        columns: [
          DataColumn(label: Text(tr('Users'))),
          DataColumn(label: Text(tr('Status'))),
          DataColumn(label: Text(tr('Last seen'))),
          DataColumn(label: Text(tr('Device'))),
          DataColumn(label: Text(tr('Address'))),
          DataColumn(label: Text(tr('Watching'))),
          DataColumn(label: Text(tr('Bandwidth'))),
          DataColumn(label: Text(tr('Last played'))),
        ],
        rows: [
          for (final u in users)
            DataRow(cells: [
              DataCell(Text('${u['display_name'] ?? u['username']}${u['admin'] == true ? tr(' (admin)') : ''}', style: const TextStyle(fontWeight: FontWeight.bold))),
              DataCell(Row(children: [
                Icon(Icons.circle, size: 10, color: u['active'] == true ? context.good : Colors.grey),
                const SizedBox(width: 6),
                Text(u['active'] == true ? tr('active') : tr('away')),
              ])),
              DataCell(Text(u['last_seen'] != null ? fmtAgo(u['last_seen'], now) : tr('not since the start'))),
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
      _chart(tr('Bandwidth · {unit}', {'unit': unit}), times, [
        _Series(tr('Local'), context.info, [for (final s in ss) s['out_bps'] == null ? null : (((s['out_bps'] as num) - ((s['out_remote_bps'] ?? 0) as num)).clamp(0, double.infinity)) * 8 / div]),
        _Series(tr('Remote'), context.warn, [for (final s in ss) s['out_bps'] == null ? null : ((s['out_remote_bps'] ?? 0) as num) * 8 / div]),
      ], null, unit: unit),
      _chart(tr('Processor'), times, [
        _Series(tr('System'), context.info, [for (final s in ss) v(s, 'cpu')]),
        _Series('Lichtspiel', context.warn, [for (final s in ss) v(s, 'cpu_server')]),
      ], 100, unit: '%'),
      _chart(tr('Memory'), times, [
        _Series(tr('System'), context.info, [for (final s in ss) v(s, 'mem_used') == null ? null : v(s, 'mem_used')! / 1e9]),
        _Series('Lichtspiel', context.warn, [for (final s in ss) v(s, 'mem_server') == null ? null : v(s, 'mem_server')! / 1e9]),
      ], memMax, unit: 'GB'),
      _chart(tr('Streams'), times, [
        _Series(tr('Playing'), context.info, [for (final s in ss) v(s, 'streams')]),
        _Series(tr('Converted'), context.warn, [for (final s in ss) v(s, 'transcodes')]),
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
            SizedBox(height: 150, width: double.infinity, child: CustomPaint(painter: _LinePainter(times, series, max, unit, context.hair2, context.faint))),
          ]),
        ),
      );

  Widget _history(Map<String, dynamic> data) {
    final r = hist;
    final users = (data['users'] as List).cast<Map>();
    final head = Row(children: [
      Expanded(child: _h(tr('Play history'))),
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
        segments: [ButtonSegment(value: false, label: Text(tr('Plays'))), ButtonSegment(value: true, label: Text(tr('Hours')))],
        selected: {hours},
        onSelectionChanged: (s) => setState(() => hours = s.first),
      ),
      const SizedBox(width: 8),
      DropdownButton<String>(
        value: user,
        items: [DropdownMenuItem(value: '', child: Text(tr('All'))), for (final u in users) DropdownMenuItem(value: '${u['id']}', child: Text('${u['display_name'] ?? u['username']}'))],
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
    final buckets = <ChartBucket>[];
    for (var t = start; !t.isAfter(day0); t = t.add(step)) {
      buckets.add(ChartBucket(t.millisecondsSinceEpoch / 1000, t.add(step).millisecondsSinceEpoch / 1000, fmtDay(t, year: false)));
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
        : '${it['title'] ?? tr('(removed)')}';
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
              Text('${hours ? tr('Hours watched') : tr('Plays')} pro ${weekly ? tr('week') : tr('day')}', style: const TextStyle(fontWeight: FontWeight.w600)),
              const Spacer(),
              Text(tr('{r} plays · {dur} watched', {'r': r['size'], 'dur': dur(r['minutes'] ?? 0)}), style: TextStyle(color: context.dim, fontSize: 12)),
            ]),
            const SizedBox(height: 8),
            SizedBox(height: 180, width: double.infinity, child: CustomPaint(painter: StackedBarPainter(buckets, context.hair2, context.faint, {'movie': context.info, 'episode': context.warn}))),
            const SizedBox(height: 6),
            Row(children: [
              Icon(Icons.circle, size: 10, color: context.info), SizedBox(width: 4), Text(tr('Movies'), style: TextStyle(fontSize: 12)), SizedBox(width: 12),
              Icon(Icons.circle, size: 10, color: context.warn), SizedBox(width: 4), Text(tr('Episodes'), style: TextStyle(fontSize: 12)), SizedBox(width: 12),
              Icon(Icons.circle, size: 10, color: Colors.grey), SizedBox(width: 4), Text(tr('Other'), style: TextStyle(fontSize: 12)),
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
                Text(tr('Watched most'), style: TextStyle(fontWeight: FontWeight.w600)),
                if (titles.isEmpty) Padding(padding: EdgeInsets.all(8), child: Text(tr('No plays in this time.'), style: TextStyle(color: context.faint))),
                for (final x in titles)
                  ListTile(
                    dense: true,
                    contentPadding: EdgeInsets.zero,
                    leading: SizedBox(width: 34, child: Image.network(api.img(x['thumb']), headers: api.auth, fit: BoxFit.cover, cacheWidth: 80, errorBuilder: (_, _, _) => Container(color: context.hair))),
                    title: Text('${x['title'] ?? tr('(removed)')}', maxLines: 1, overflow: TextOverflow.ellipsis),
                    subtitle: Text([x['year'], if (x['type'] == 'show') tr('Show')].where((e) => e != null).join(' · ')),
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
                Text(tr('Top users'), style: TextStyle(fontWeight: FontWeight.w600)),
                if (topUsers.isEmpty) Padding(padding: EdgeInsets.all(8), child: Text(tr('No plays in this time.'), style: TextStyle(color: context.faint))),
                for (final x in topUsers)
                  ListTile(dense: true, contentPadding: EdgeInsets.zero, title: Text('${x['display_name'] ?? x['username'] ?? '?'}'), trailing: Text(tr('{x} plays · {dur}', {'x': x['plays'], 'dur': dur(x['minutes'] ?? 0)}), style: TextStyle(fontSize: 12, color: context.dim))),
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
            Text(tr('Recent plays'), style: TextStyle(fontWeight: FontWeight.w600)),
            if (recent.isEmpty) Padding(padding: EdgeInsets.all(8), child: Text(tr('No plays in this time.'), style: TextStyle(color: context.faint))),
            for (final e in recent)
              ListTile(
                dense: true,
                contentPadding: EdgeInsets.zero,
                title: Text(what(Map.from(e['item'] ?? const {})), maxLines: 1, overflow: TextOverflow.ellipsis),
                subtitle: Text('${fmtDateTime(e['at'])} · ${e['user'] ?? '?'}'),
                trailing: Text(e['manual'] == true ? tr('marked by hand') : '${dur(e['minutes'] ?? 0)}${e['live'] == true ? tr(' · playing now') : ''}', style: TextStyle(fontSize: 12, color: context.dim)),
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
  final Color gridColor, textColor;
  _LinePainter(this.times, this.series, this.max, this.unit, this.gridColor, this.textColor);

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
    final grid = Paint()..color = gridColor..strokeWidth = 1;
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
      _text(canvas, long ? fmtDay(t, year: false) : '${p(t.hour)}:${p(t.minute)}', Offset((left + w * f - (f == 1 ? 30 : f == 0.5 ? 14 : 0)), h + 3), 11);
    }
  }

  void _text(Canvas c, String s, Offset o, double size) {
    final tp = TextPainter(text: TextSpan(text: s, style: TextStyle(color: textColor, fontSize: size)), textDirection: TextDirection.ltr)..layout();
    tp.paint(c, o);
  }

  @override
  bool shouldRepaint(_LinePainter old) => true;
}

/// One bar of [StackedBarPainter]: a time span and its parts (movie, episode, other …).
class ChartBucket {
  final double from, to;
  final String label;
  final Map<String, num> parts = {};
  ChartBucket(this.from, this.to, this.label);
}

/// Stacked bars per bucket, the parts in the order of [keys] (one without a colour is grey).
class StackedBarPainter extends CustomPainter {
  final List<ChartBucket> buckets;
  final List<String> keys;
  final Color gridColor, textColor;
  final Map<String, Color> colors;
  StackedBarPainter(this.buckets, this.gridColor, this.textColor, this.colors, {this.keys = const ['movie', 'episode', 'other']});

  @override
  void paint(Canvas canvas, Size size) {
    const left = 36.0, bottom = 18.0;
    final w = size.width - left, h = size.height - bottom;
    double total(ChartBucket b) => b.parts.values.fold<double>(0, (a, v) => a + v.toDouble());
    var top = buckets.fold<double>(0, (m, b) => total(b) > m ? total(b) : m);
    if (top <= 0) top = 1;
    final grid = Paint()..color = gridColor;
    for (var i = 0; i <= 4; i++) {
      final y = h - h * i / 4;
      canvas.drawLine(Offset(left, y), Offset(size.width, y), grid);
      final tp = TextPainter(text: TextSpan(text: (top * i / 4).toStringAsFixed(top < 10 ? 1 : 0), style: TextStyle(color: textColor, fontSize: 11)), textDirection: TextDirection.ltr)..layout();
      tp.paint(canvas, Offset(0, y - 6));
    }
    if (buckets.isEmpty) return;
    final bw = w / buckets.length;
    for (var i = 0; i < buckets.length; i++) {
      var y = h;
      for (final k in keys) {
        final v = buckets[i].parts[k] ?? 0;
        if (v <= 0) continue;
        final bh = h * v.toDouble() / top;
        canvas.drawRect(Rect.fromLTWH(left + bw * i + bw * .12, y - bh, bw * .76, bh), Paint()..color = colors[k] ?? Colors.grey);
        y -= bh;
      }
    }
    final step = (buckets.length / 6).ceil().clamp(1, 1000);
    for (var i = 0; i < buckets.length; i += step) {
      final tp = TextPainter(text: TextSpan(text: buckets[i].label, style: TextStyle(color: textColor, fontSize: 11)), textDirection: TextDirection.ltr)..layout();
      tp.paint(canvas, Offset(left + bw * i, h + 3));
    }
  }

  @override
  bool shouldRepaint(StackedBarPainter old) => true;
}
