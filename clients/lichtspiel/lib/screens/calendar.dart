import 'package:flutter/material.dart';
import '../api.dart';
import '../theme.dart';
import '../widgets/states.dart';
import 'detail.dart';

/// "Coming soon": next episodes of the shows in the libraries and what Radarr and Sonarr expect
/// (GET /api/upcoming), as a row on the start screen and as a calendar by day.

const _weekdays = ['Montag', 'Dienstag', 'Mittwoch', 'Donnerstag', 'Freitag', 'Samstag', 'Sonntag'];
const _months = ['Januar', 'Februar', 'März', 'April', 'Mai', 'Juni', 'Juli', 'August', 'September', 'Oktober', 'November', 'Dezember'];
const _notes = {'in cinemas': 'Im Kino', 'digital release': 'Digital erhältlich', 'on disc': 'Auf Disc'};

/// "2026-11-28" → Heute / Morgen / the weekday within a week / "Sa., 28.11." later;
/// [long]: "Morgen · Sonntag, 22. November".
String comingDay(String date, {bool long = false, DateTime? today}) {
  final p = date.split('-').map(int.tryParse).toList();
  if (p.length != 3 || p.contains(null)) return date;
  final at = DateTime(p[0]!, p[1]!, p[2]!);
  final now = today ?? DateTime.now();
  final days = at.difference(DateTime(now.year, now.month, now.day)).inHours ~/ 24;
  final wd = _weekdays[at.weekday - 1];
  final full = long ? '$wd, ${at.day}. ${_months[at.month - 1]}' : '${wd.substring(0, 2)}., ${at.day}.${at.month}.';
  if (days == 0) return long ? 'Heute · $full' : 'Heute';
  if (days == 1) return long ? 'Morgen · $full' : 'Morgen';
  if (days > 1 && days < 7 && !long) return wd;
  return full;
}

/// "S4 E1 · Anfang", or for a movie "2026 · Im Kino".
String comingWhat(Map<String, dynamic> e) {
  if (e['kind'] == 'episode') {
    return [if (e['season'] != null && e['episode'] != null) 'S${e['season']} E${e['episode']}', '${e['name'] ?? ''}'].where((s) => s.isNotEmpty).join(' · ');
  }
  final note = '${e['note'] ?? ''}';
  return [if (e['year'] != null) '${e['year']}', _notes[note] ?? note].where((s) => s.isNotEmpty).join(' · ');
}

/// One card a show (its next episode, "+2" when more follow), every movie on its own.
List<(Map<String, dynamic>, int)> comingCards(List<Map<String, dynamic>> entries) {
  final out = <(Map<String, dynamic>, int)>[];
  final at = <String, int>{};
  for (final e in entries) {
    final k = e['show_id'] != null ? 's${e['show_id']}' : (e['kind'] == 'episode' ? 'e${e['title']}' : null);
    if (k != null && at.containsKey(k)) {
      final i = at[k]!;
      out[i] = (out[i].$1, out[i].$2 + 1);
      continue;
    }
    if (k != null) at[k] = out.length;
    out.add((e, 0));
  }
  return out;
}

Widget _picture(Api api, Map<String, dynamic> e, double width) {
  final thumb = e['thumb'] as String?, poster = e['poster'] as String?;
  if (thumb != null) return Image.network(api.img(thumb), headers: api.auth, fit: BoxFit.cover, cacheWidth: (width * 2).round(), errorBuilder: (_, _, _) => const SizedBox.shrink());
  if (poster != null && poster.startsWith('https://')) return Image.network(poster, fit: BoxFit.cover, cacheWidth: (width * 2).round(), errorBuilder: (_, _, _) => const SizedBox.shrink());
  return const Center(child: Icon(Icons.event, size: 40));
}

void _open(BuildContext context, Api api, Map<String, dynamic> e) {
  if (e['show_id'] == null) return;
  Navigator.push(context, MaterialPageRoute(builder: (_) => DetailScreen(api: api, id: '${e['show_id']}')));
}

/// The start screen's row "Demnächst" with a link to the calendar.
class ComingRow extends StatelessWidget {
  final Api api;
  final List<Map<String, dynamic>> entries;
  const ComingRow({super.key, required this.api, required this.entries});

  @override
  Widget build(BuildContext context) {
    final cards = comingCards(entries);
    return Column(crossAxisAlignment: CrossAxisAlignment.start, children: [
      Padding(
        padding: const EdgeInsets.fromLTRB(16, 12, 8, 0),
        child: Row(children: [
          Text('Demnächst', style: Theme.of(context).textTheme.titleMedium),
          const SizedBox(width: 8),
          TextButton(onPressed: () => Navigator.push(context, MaterialPageRoute(builder: (_) => CalendarScreen(api: api))), child: const Text('Kalender ›')),
        ]),
      ),
      SizedBox(
        height: 280,
        child: ListView.separated(
          scrollDirection: Axis.horizontal,
          padding: const EdgeInsets.symmetric(horizontal: 16),
          itemCount: cards.length,
          separatorBuilder: (_, _) => const SizedBox(width: 12),
          itemBuilder: (context, i) {
            final (e, more) = cards[i];
            final what = comingWhat(e);
            return SizedBox(
              width: 140,
              child: InkWell(
                borderRadius: BorderRadius.circular(6),
                onTap: e['show_id'] != null ? () => _open(context, api, e) : null,
                child: Column(crossAxisAlignment: CrossAxisAlignment.start, children: [
                  AspectRatio(
                    aspectRatio: 2 / 3,
                    child: ClipRRect(
                      borderRadius: BorderRadius.circular(6),
                      child: Stack(fit: StackFit.expand, children: [
                        Container(color: context.hair),
                        _picture(api, e, 140),
                        if (more > 0)
                          Positioned(top: 6, right: 6, child: Container(padding: const EdgeInsets.symmetric(horizontal: 7), decoration: BoxDecoration(color: context.accent, borderRadius: BorderRadius.circular(10)), child: Text('+$more', style: TextStyle(color: context.onAccent, fontSize: 12, fontWeight: FontWeight.w700)))),
                      ]),
                    ),
                  ),
                  const SizedBox(height: 6),
                  Text(comingDay('${e['date']}'), maxLines: 1, overflow: TextOverflow.ellipsis, style: const TextStyle(fontWeight: FontWeight.w600)),
                  Text('${e['title']}${what.isEmpty ? '' : ' · $what'}', maxLines: 2, overflow: TextOverflow.ellipsis, style: TextStyle(fontSize: 12, color: context.dim)),
                ]),
              ),
            );
          },
        ),
      ),
    ]);
  }
}

/// Everything for the next 90 days, by day.
class CalendarScreen extends StatefulWidget {
  final Api api;
  const CalendarScreen({super.key, required this.api});
  @override
  State<CalendarScreen> createState() => _CalendarScreenState();
}

class _CalendarScreenState extends State<CalendarScreen> {
  late Future<List<Map<String, dynamic>>> data = widget.api.upcoming(days: 90);

  Future<void> reload() async {
    setState(() => data = widget.api.upcoming(days: 90));
    await data;
  }

  @override
  Widget build(BuildContext context) {
    final api = widget.api;
    return Scaffold(
      appBar: AppBar(title: const Text('Kalender')),
      body: FutureBuilder<List<Map<String, dynamic>>>(
        future: data,
        builder: (context, snap) {
          if (snap.hasError) return ErrorView(error: snap.error!, onRetry: reload);
          if (!snap.hasData) return const Center(child: CircularProgressIndicator());
          final days = <String, List<Map<String, dynamic>>>{};
          for (final e in snap.data!) {
            days.putIfAbsent('${e['date']}', () => []).add(e);
          }
          return RefreshIndicator(
            onRefresh: reload,
            child: ListView(padding: const EdgeInsets.all(16), children: [
              Text('Neue Folgen der Serien in deinen Mediatheken, einmal am Tag nachgesehen, und was Radarr und Sonarr erwarten, wenn das Wunschlisten-Plugin an ist.',
                  style: TextStyle(color: context.dim)),
              if (days.isEmpty) const Padding(padding: EdgeInsets.all(32), child: Center(child: Text('In den nächsten 90 Tagen kommt nichts.'))),
              for (final MapEntry(key: date, value: list) in days.entries) ...[
                Padding(padding: const EdgeInsets.fromLTRB(0, 20, 0, 8), child: Text(comingDay(date, long: true), style: Theme.of(context).textTheme.titleMedium)),
                for (final e in list)
                  Card(
                    margin: const EdgeInsets.only(bottom: 8),
                    child: ListTile(
                      leading: SizedBox(width: 40, height: 60, child: ClipRRect(borderRadius: BorderRadius.circular(4), child: _picture(api, e, 40))),
                      title: Text('${e['title']}'),
                      subtitle: Text(comingWhat(e)),
                      onTap: e['show_id'] != null ? () => _open(context, api, e) : null,
                    ),
                  ),
              ],
            ]),
          );
        },
      ),
    );
  }
}
