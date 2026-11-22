import 'package:flutter/material.dart';
import '../api.dart';
import '../theme.dart';
import 'dashboard.dart';
import 'home.dart';
import '../i18n.dart';

/// Statistik, like the web page (#/stats): one's own viewing (an admin: everyone's or one person's)
/// over 30 or 90 days, a year or everything: totals, per day, week or month, the times of day, the most
/// watched shows, movies and genres; for an admin also who watches how much and the load on the server
/// per hour of the day (from the hour averages of the last 30 days).
class StatsScreen extends StatefulWidget {
  final Api api;
  final bool admin;
  const StatsScreen({super.key, required this.api, required this.admin});
  @override
  State<StatsScreen> createState() => _StatsScreenState();
}

List<String> get _weekdays => [tr('Mon'), tr('Tue'), tr('Wed'), tr('Thu'), tr('Fri'), tr('Sat'), tr('Sun')];

/// The time span a statistic covers ([days] back, `span` days with viewing in them) splits into days,
/// weeks (beyond 90 days) or months (beyond 400), counted in local time.
List<ChartBucket> statBuckets(DateTime now, int days, int span) {
  final unit = span > 400 ? 'month' : span > 90 ? 'week' : 'day';
  final day0 = DateTime(now.year, now.month, now.day);
  var start = DateTime(day0.year, day0.month, day0.day - ((span < days ? span : days) - 1));
  if (unit == 'week') start = DateTime(start.year, start.month, start.day - (start.weekday - 1));
  if (unit == 'month') start = DateTime(start.year, start.month, 1);
  final out = <ChartBucket>[];
  for (var d = start; !d.isAfter(day0);) {
    final next = unit == 'month' ? DateTime(d.year, d.month + 1, 1) : DateTime(d.year, d.month, d.day + (unit == 'week' ? 7 : 1));
    out.add(ChartBucket(d.millisecondsSinceEpoch / 1000, next.millisecondsSinceEpoch / 1000, unit == 'month' ? '${d.month}/${d.year % 100}' : fmtDay(d, year: false)));
    d = next;
  }
  return out;
}

class _StatsScreenState extends State<StatsScreen> {
  int days = 365;
  bool hours = true;
  String user = '';
  List<Map> people = [];
  Map<String, dynamic>? data;
  String? error;
  List<double>? cpu; // processor per hour of the day, % (admin)
  List<double> streams = [], converted = [];

  Api get api => widget.api;

  @override
  void initState() {
    super.initState();
    load();
    if (widget.admin) {
      api.get('/api/users').then((r) {
        if (mounted) setState(() => people = ((r is Map ? r['users'] : r) as List? ?? const []).cast<Map>());
      }).catchError((_) {});
      loadServer();
    }
  }

  Future<void> load() async {
    setState(() => error = null);
    try {
      final r = Map<String, dynamic>.from(await api.get('/api/stats/plays?days=$days${widget.admin && user.isNotEmpty ? '&user=$user' : ''}'));
      if (mounted) setState(() => data = r);
    } catch (e) {
      if (mounted) setState(() => error = '$e');
    }
  }

  Future<void> loadServer() async {
    try {
      final r = await api.get('/api/dashboard/stats?range=30d');
      final sum = List.filled(24, 0.0), n = List.filled(24, 0), st = List.filled(24, 0.0), tc = List.filled(24, 0.0), k = List.filled(24, 0);
      for (final s in (r['samples'] as List? ?? const [])) {
        if (s is! Map || s['streams'] == null) continue;
        final hh = DateTime.fromMillisecondsSinceEpoch(((s['at'] as num) * 1000).toInt()).hour;
        k[hh]++;
        st[hh] += (s['streams'] as num).toDouble();
        tc[hh] += ((s['transcodes'] ?? 0) as num).toDouble();
        if (s['cpu'] is num) {
          sum[hh] += (s['cpu'] as num).toDouble();
          n[hh]++;
        }
      }
      if (!mounted || !k.any((x) => x > 0)) return;
      setState(() {
        cpu = [for (var i = 0; i < 24; i++) n[i] > 0 ? sum[i] / n[i] : 0];
        streams = [for (var i = 0; i < 24; i++) k[i] > 0 ? st[i] / k[i] : 0];
        converted = [for (var i = 0; i < 24; i++) k[i] > 0 ? tc[i] / k[i] : 0];
      });
    } catch (_) {}
  }

  String dur(num mins) => mins >= 60 ? '${(mins / 60).floor()} h ${(mins % 60).round()} min' : '${mins.round()} min';
  String num1(num v) => v < 10 && v != v.roundToDouble() ? decimal(v.toStringAsFixed(1)) : v.round().toString();

  Widget card(String title, List<Widget> kids, {String? note}) => Card(
        child: Padding(
          padding: const EdgeInsets.all(12),
          child: Column(crossAxisAlignment: CrossAxisAlignment.start, children: [
            Row(children: [
              Expanded(child: Text(title, style: const TextStyle(fontWeight: FontWeight.w600))),
              if (note != null) Text(note, style: TextStyle(color: context.dim, fontSize: 12)),
            ]),
            const SizedBox(height: 8),
            ...kids,
          ]),
        ),
      );

  Widget tile(String label, String value, [String? note]) => SizedBox(
        width: 190,
        child: Card(
          child: Padding(
            padding: const EdgeInsets.all(12),
            child: Column(crossAxisAlignment: CrossAxisAlignment.start, children: [
              Text(label, style: TextStyle(color: context.dim, fontSize: 13)),
              Text(value, style: const TextStyle(fontSize: 24, fontWeight: FontWeight.w600)),
              if (note != null) Text(note, style: TextStyle(color: context.faint, fontSize: 12)),
            ]),
          ),
        ),
      );

  Widget none() => Padding(padding: const EdgeInsets.all(8), child: Text(tr('No plays in this time.'), style: TextStyle(color: context.faint)));

  List<Map> sorted(dynamic list) {
    final l = (list as List? ?? const []).cast<Map>().toList();
    l.sort((a, b) => hours ? ((b['minutes'] ?? 0) as num).compareTo((a['minutes'] ?? 0) as num) : ((b['plays'] ?? 0) as num).compareTo((a['plays'] ?? 0) as num));
    return l;
  }

  String amount(Map x) => hours ? dur((x['minutes'] ?? 0) as num) : '${x['plays']}×';

  Widget titleList(dynamic list) {
    final l = sorted(list);
    if (l.isEmpty) return none();
    return Column(children: [
      for (final x in l)
        ListTile(
          dense: true,
          contentPadding: EdgeInsets.zero,
          leading: SizedBox(width: 34, child: Image.network(api.img(x['thumb']), headers: api.auth, fit: BoxFit.cover, cacheWidth: 80, errorBuilder: (_, _, _) => Container(color: context.hair))),
          title: Text('${x['title'] ?? tr('(removed)')}', maxLines: 1, overflow: TextOverflow.ellipsis),
          subtitle: x['year'] == null ? null : Text('${x['year']}'),
          trailing: Text(amount(x), style: TextStyle(fontSize: 12, color: context.dim)),
          onTap: x['id'] == null ? null : () => openItem(context, api, {'ratingKey': x['id']}),
        ),
    ]);
  }

  @override
  Widget build(BuildContext context) {
    return Scaffold(
      appBar: AppBar(title: Text(tr('Statistics'))),
      body: RefreshIndicator(
        onRefresh: load,
        child: ListView(padding: const EdgeInsets.all(12), children: [
          // On a phone the choices scroll sideways.
          SingleChildScrollView(scrollDirection: Axis.horizontal, child: Row(children: [
            SegmentedButton<int>(
              showSelectedIcon: false,
              segments: [ButtonSegment(value: 30, label: Text('30 T')), ButtonSegment(value: 90, label: Text('90 T')), ButtonSegment(value: 365, label: Text('1 J')), ButtonSegment(value: 3660, label: Text(tr('Everything')))],
              selected: {days},
              onSelectionChanged: (s) {
                days = s.first;
                load();
              },
            ),
            SegmentedButton<bool>(
              showSelectedIcon: false,
              segments: [ButtonSegment(value: false, label: Text(tr('Plays'))), ButtonSegment(value: true, label: Text(tr('Hours')))],
              selected: {hours},
              onSelectionChanged: (s) => setState(() => hours = s.first),
            ),
            if (people.isNotEmpty)
              DropdownButton<String>(
                value: user,
                items: [DropdownMenuItem(value: '', child: Text(tr('All'))), for (final u in people) DropdownMenuItem(value: '${u['id']}', child: Text('${u['display_name'] ?? u['username']}'))],
                onChanged: (v) {
                  user = v ?? '';
                  load();
                },
              ),
          ].expand((w) => [w, const SizedBox(width: 8)]).toList())),
          const SizedBox(height: 12),
          if (error != null) Text(error!, style: TextStyle(color: context.bad)) else if (data == null) const Center(child: CircularProgressIndicator()) else ...body(data!),
        ]),
      ),
    );
  }

  List<Widget> body(Map<String, dynamic> r) {
    final plays = (r['plays'] as List? ?? const []);
    double add(List p) => hours ? ((p[3] ?? 0) as num) / 60 : 1;
    String fmt(double v) => hours ? '${num1(v)} h' : num1(v);
    final first = plays.isNotEmpty ? (plays.first[0] as num) : ((r['from'] ?? 0) as num);
    final from = ((r['from'] ?? 0) as num) > first ? (r['from'] as num) : first;
    final span = ((DateTime.now().millisecondsSinceEpoch / 1000 - from) / 86400).ceil().clamp(1, 100000);
    final buckets = statBuckets(DateTime.now(), days, span);
    final unit = span > 400 ? tr('month') : span > 90 ? tr('week') : tr('day');
    final grid = List.generate(7, (_) => List.filled(24, 0.0));
    final perDay = <String, double>{};
    var movies = 0, episodes = 0;
    for (final p in plays) {
      final at = p[0] as num;
      final d = DateTime.fromMillisecondsSinceEpoch((at * 1000).toInt());
      final b = buckets.where((x) => at >= x.from && at < x.to).firstOrNull;
      if (b != null) b.parts['${p[2]}'] = (b.parts['${p[2]}'] ?? 0) + add(p);
      grid[d.weekday - 1][d.hour] += add(p);
      final key = fmtDay(d);
      perDay[key] = (perDay[key] ?? 0) + add(p);
      if (p[2] == 'movie') movies++;
      if (p[2] == 'episode') episodes++;
    }
    final best = perDay.entries.fold<MapEntry<String, double>?>(null, (m, e) => m == null || e.value > m.value ? e : m);
    final byHour = List.generate(24, (h) => grid.fold<double>(0, (a, row) => a + row[h]));
    final peak = byHour.indexOf(byHour.reduce((a, b) => a > b ? a : b));
    String hh(int h) => '${h.toString().padLeft(2, '0')}:00';
    final colors = {'movie': context.info, 'episode': context.warn};
    final wide = MediaQuery.sizeOf(context).width > 900;
    Widget pair(Widget a, Widget? b) => wide && b != null
        ? Row(crossAxisAlignment: CrossAxisAlignment.start, children: [Expanded(child: a), const SizedBox(width: 12), Expanded(child: b)])
        : Column(children: [a, ?b]);
    return [
      Wrap(spacing: 8, runSpacing: 8, children: [
        tile(tr('Watched'), dur((r['minutes'] ?? 0) as num), tr('{r} plays', {'r': r['size'] ?? 0})),
        tile(tr('Movies'), '$movies'),
        tile(tr('Episodes'), '$episodes'),
        tile(tr('Days with watching'), '${perDay.length}', best == null ? null : tr('Most on {key}: {fmt}', {'key': best.key, 'fmt': fmt(best.value)})),
        if (plays.isNotEmpty) tile(tr('Favourite time'), hh(peak), tr('{hh} to {hh2}', {'hh': hh(peak), 'hh2': hh((peak + 1) % 24)})),
      ]),
      const SizedBox(height: 8),
      card(tr('{hours} per {unit}', {'hours': hours ? tr('Hours watched') : tr('Plays'), 'unit': unit}), [
        SizedBox(height: 180, width: double.infinity, child: CustomPaint(painter: StackedBarPainter(buckets, context.hair2, context.faint, colors))),
        const SizedBox(height: 6),
        Row(children: [
          Icon(Icons.circle, size: 10, color: context.info), const SizedBox(width: 4), Text(tr('Movies'), style: TextStyle(fontSize: 12)), const SizedBox(width: 12),
          Icon(Icons.circle, size: 10, color: context.warn), const SizedBox(width: 4), Text(tr('Episodes'), style: TextStyle(fontSize: 12)), const SizedBox(width: 12),
          const Icon(Icons.circle, size: 10, color: Colors.grey), const SizedBox(width: 4), Text(tr('Other'), style: TextStyle(fontSize: 12)),
        ]),
      ]),
      card(tr('Times of day'), [
        SizedBox(height: 7 * 22 + 20, width: double.infinity, child: CustomPaint(painter: HeatPainter(grid, context.info, context.hair, context.faint))),
      ], note: hours ? tr('Hours watched') : tr('Plays')),
      pair(card(tr('Most watched shows'), [titleList(r['shows'])]), card(tr('Most watched movies'), [titleList(r['movies'])])),
      pair(
        card(tr('Genres'), [
          if (sorted(r['genres']).isEmpty) none(),
          for (final g in sorted(r['genres'])) ListTile(dense: true, contentPadding: EdgeInsets.zero, title: Text('${g['genre']}'), trailing: Text(amount(g), style: TextStyle(fontSize: 12, color: context.dim))),
        ]),
        widget.admin && user.isEmpty
            ? card(tr('Who watches how much'), [
                if (sorted(r['users']).isEmpty) none(),
                for (final x in sorted(r['users']))
                  ListTile(dense: true, contentPadding: EdgeInsets.zero, title: Text('${x['display_name'] ?? x['username'] ?? '?'}'),
                      subtitle: Text(tr('{x} plays · {dur}', {'x': x['plays'], 'dur': dur((x['minutes'] ?? 0) as num)}), style: TextStyle(fontSize: 12, color: context.dim))),
              ])
            : null,
      ),
      if (widget.admin)
        card(tr('Load on the server by time of day'), [
          if (cpu == null)
            Text(tr('Nothing measured yet.'), style: TextStyle(color: context.faint))
          else ...[
            SizedBox(
              height: 160,
              width: double.infinity,
              child: CustomPaint(painter: StackedBarPainter([for (var i = 0; i < 24; i++) ChartBucket(i.toDouble(), i + 1.0, '$i')..parts['cpu'] = cpu![i]], context.hair2, context.faint, {'cpu': context.good}, keys: const ['cpu'])),
            ),
            const SizedBox(height: 6),
            Text(tr('Processor in % · most streams at {hh} (on average {num1}, {num12} of them converted)', {'hh': hh(streams.indexOf(streams.reduce((a, b) => a > b ? a : b))), 'num1': num1(streams.reduce((a, b) => a > b ? a : b)), 'num12': num1(converted[streams.indexOf(streams.reduce((a, b) => a > b ? a : b))])}),
                style: TextStyle(fontSize: 12, color: context.dim)),
          ],
        ], note: tr('Average of the last 30 days')),
    ];
  }
}

/// Weekdays (rows) by hours (columns): the stronger the colour, the more.
class HeatPainter extends CustomPainter {
  final List<List<double>> grid;
  final Color color, empty, text;
  HeatPainter(this.grid, this.color, this.empty, this.text);

  @override
  void paint(Canvas canvas, Size size) {
    const left = 28.0, bottom = 18.0;
    final cw = (size.width - left) / 24, ch = (size.height - bottom) / 7;
    final max = grid.expand((r) => r).fold<double>(0, (a, v) => v > a ? v : a);
    for (var d = 0; d < 7; d++) {
      final tp = TextPainter(text: TextSpan(text: _weekdays[d], style: TextStyle(color: text, fontSize: 11)), textDirection: TextDirection.ltr)..layout();
      tp.paint(canvas, Offset(0, ch * d + ch / 2 - tp.height / 2));
      for (var h = 0; h < 24; h++) {
        final v = grid[d][h];
        final paint = Paint()..color = v > 0 ? color.withValues(alpha: 0.18 + 0.82 * v / max) : empty;
        canvas.drawRRect(RRect.fromRectAndRadius(Rect.fromLTWH(left + cw * h + 1, ch * d + 1, cw - 2, ch - 2), const Radius.circular(3)), paint);
      }
    }
    for (var h = 0; h < 24; h += cw < 22 ? 3 : 2) {
      final tp = TextPainter(text: TextSpan(text: '$h', style: TextStyle(color: text, fontSize: 11)), textDirection: TextDirection.ltr)..layout();
      tp.paint(canvas, Offset(left + cw * h + cw / 2 - tp.width / 2, size.height - bottom + 3));
    }
  }

  @override
  bool shouldRepaint(HeatPainter old) => true;
}
