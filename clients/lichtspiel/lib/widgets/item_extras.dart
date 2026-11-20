import 'package:flutter/material.dart';
import '../api.dart';
import '../widgets/poster.dart';
import '../theme.dart';

const _problems = {
  'playback': 'Spielt nicht ab / bricht ab',
  'video': 'Bildproblem',
  'audio': 'Tonproblem',
  'subtitles': 'Untertitelproblem',
  'metadata': 'Falsche Details oder falsches Poster',
  'other': 'Etwas anderes',
};

String _when(dynamic at) {
  final n = at is num ? at.toInt() : int.tryParse('$at') ?? 0;
  final d = DateTime.fromMillisecondsSinceEpoch(n < 100000000000 ? n * 1000 : n);
  String two(int x) => x.toString().padLeft(2, '0');
  return '${two(d.day)}.${two(d.month)}.${d.year} ${two(d.hour)}:${two(d.minute)}';
}

/// "Verlauf": when the title was watched (the owner sees everyone's, others their own).
Future<void> historyDialog(BuildContext context, Api api, Map<String, dynamic> item) async {
  final id = '${item['ratingKey'] ?? item['id']}';
  final playable = const {'movie', 'episode', 'track', 'clip'}.contains(item['type']);
  List<Map<String, dynamic>> rows;
  try {
    final r = await api.get('/library/metadata/$id/history');
    rows = [for (final e in (r['history'] as List? ?? const [])) Map<String, dynamic>.from(e)];
  } catch (e) {
    if (context.mounted) ScaffoldMessenger.of(context).showSnackBar(SnackBar(content: Text('$e')));
    return;
  }
  if (!context.mounted) return;
  String what(Map x) => x['type'] == 'episode' ? 'S${x['parentIndex'] ?? 0}E${x['index'] ?? 0} · ${x['title']}' : '${x['title']}';
  await showDialog(
    context: context,
    builder: (ctx) => AlertDialog(
      title: Text('Verlauf: ${item['title']}'),
      content: SizedBox(
        width: 460,
        child: rows.isEmpty
            ? const Text('Noch nicht gesehen.')
            : ListView(shrinkWrap: true, children: [
                for (final e in rows)
                  ListTile(
                    dense: true,
                    contentPadding: EdgeInsets.zero,
                    title: Text('${_when(e['at'])}${Poster.admin && e['user'] != null ? ' · ${e['user']}' : ''}'),
                    subtitle: Text([if (!playable && e['item'] is Map) what(e['item']), e['manual'] == true ? 'als gesehen markiert' : 'gesehen'].join(' · ')),
                  ),
              ]),
      ),
      actions: [TextButton(onPressed: () => Navigator.pop(ctx), child: const Text('Schließen'))],
    ),
  );
}

/// "Problem melden": the owner of the server sees the report.
Future<void> reportDialog(BuildContext context, Api api, Map<String, dynamic> item) async {
  final id = '${item['ratingKey'] ?? item['id']}';
  var kind = 'playback';
  final note = TextEditingController();
  final send = await showDialog<bool>(
    context: context,
    builder: (ctx) => StatefulBuilder(
      builder: (ctx, set) => AlertDialog(
        title: const Text('Problem melden'),
        content: SizedBox(
          width: 420,
          child: Column(mainAxisSize: MainAxisSize.min, crossAxisAlignment: CrossAxisAlignment.start, children: [
            Text('Der Besitzer dieses Servers sieht deine Meldung.', style: TextStyle(color: context.faint)),
            const SizedBox(height: 8),
            DropdownButton<String>(
              value: kind,
              isExpanded: true,
              onChanged: (v) => set(() => kind = v ?? 'playback'),
              items: [for (final e in _problems.entries) DropdownMenuItem(value: e.key, child: Text(e.value))],
            ),
            TextField(controller: note, minLines: 3, maxLines: 6, decoration: const InputDecoration(labelText: 'Was passiert? (optional)')),
          ]),
        ),
        actions: [
          TextButton(onPressed: () => Navigator.pop(ctx, false), child: const Text('Abbrechen')),
          FilledButton(onPressed: () => Navigator.pop(ctx, true), child: const Text('Senden')),
        ],
      ),
    ),
  );
  final text = note.text;
  note.dispose();
  if (send != true) return;
  try {
    await api.post('/library/metadata/$id/report', {'kind': kind, 'note': text});
    if (context.mounted) ScaffoldMessenger.of(context).showSnackBar(const SnackBar(content: Text('Danke – das Problem wurde gemeldet')));
  } catch (e) {
    if (context.mounted) ScaffoldMessenger.of(context).showSnackBar(SnackBar(content: Text('$e')));
  }
}

String _clock(num ms) {
  final t = ms ~/ 1000, h = t ~/ 3600, m = (t % 3600) ~/ 60, s = t % 60;
  String two(int n) => n.toString().padLeft(2, '0');
  return h > 0 ? '$h:${two(m)}:${two(s)}' : '$m:${two(s)}';
}

/// Admin: where the end credits start; the video counts as watched from there.
Future<void> creditsDialog(BuildContext context, Api api, Map<String, dynamic> item) async {
  final id = '${item['ratingKey'] ?? item['id']}';
  Map r;
  try {
    r = await api.get('/library/metadata/$id/markers');
  } catch (e) {
    if (context.mounted) ScaffoldMessenger.of(context).showSnackBar(SnackBar(content: Text('$e')));
    return;
  }
  if (!context.mounted) return;
  final credits = (r['credits'] as List?) ?? const [];
  final inp = TextEditingController(text: credits.isEmpty ? '' : _clock(credits.first as num));
  const sources = {
    'manual': 'von Hand gesetzt',
    'chapters': 'aus einem Kapitel „Credits“ in der Datei',
    'picture': 'im Bild gefunden (dunkler Schirm mit Text am Ende)',
    'none': 'nichts gefunden',
  };
  int? parse(String v) {
    final p = v.trim().split(':').map(int.tryParse).toList();
    if (v.trim().isEmpty || p.any((x) => x == null)) return null;
    return p.fold<int>(0, (a, b) => a * 60 + b!) * 1000;
  }

  Future<void> save(BuildContext ctx, Object? value) async {
    try {
      await api.put('/library/metadata/$id/markers', {'credits': value});
      if (ctx.mounted) Navigator.pop(ctx);
    } catch (e) {
      if (ctx.mounted) ScaffoldMessenger.of(ctx).showSnackBar(SnackBar(content: Text('$e')));
    }
  }

  await showDialog(
    context: context,
    builder: (ctx) => AlertDialog(
      title: const Text('Abspann'),
      content: SizedBox(
        width: 420,
        child: Column(mainAxisSize: MainAxisSize.min, crossAxisAlignment: CrossAxisAlignment.start, children: [
          Text('Abspann-Marke: ${sources['${r['source']}'] ?? r['source']}. Das Video gilt ab ${_clock((r['watchedAt'] ?? 0) as num)} von ${_clock((r['duration'] ?? 0) as num)} als gesehen.', style: TextStyle(color: context.faint)),
          const SizedBox(height: 8),
          TextField(controller: inp, decoration: const InputDecoration(labelText: 'Abspann beginnt bei (Std:Min:Sek)', hintText: 'z. B. 1:52:30')),
        ]),
      ),
      actions: [
        if (r['source'] == 'manual') TextButton(onPressed: () => save(ctx, null), child: const Text('Kapitel der Datei nutzen')),
        TextButton(
          onPressed: () async {
            try {
              await api.post('/library/metadata/$id/credits');
              if (ctx.mounted) {
                Navigator.pop(ctx);
                ScaffoldMessenger.of(ctx).showSnackBar(const SnackBar(content: Text('Abspann wird erkannt…')));
              }
            } catch (e) {
              if (ctx.mounted) ScaffoldMessenger.of(ctx).showSnackBar(SnackBar(content: Text('$e')));
            }
          },
          child: const Text('Neu erkennen'),
        ),
        TextButton(onPressed: () => Navigator.pop(ctx), child: const Text('Abbrechen')),
        FilledButton(
          onPressed: () {
            final ms = parse(inp.text);
            if (ms == null) {
              ScaffoldMessenger.of(ctx).showSnackBar(const SnackBar(content: Text('Gib eine Zeit wie 1:52:30 ein')));
              return;
            }
            save(ctx, [ms]);
          },
          child: const Text('Speichern'),
        ),
      ],
    ),
  );
  inp.dispose();
}
