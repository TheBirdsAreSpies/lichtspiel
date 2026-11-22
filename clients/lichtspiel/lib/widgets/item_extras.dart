import 'package:flutter/material.dart';
import '../api.dart';
import '../clock.dart';
import '../widgets/poster.dart';
import '../theme.dart';
import 'states.dart';
import '../i18n.dart';

Map<String, String> get _problems => {
  'playback': tr('Does not play / stops'),
  'video': tr('Picture problem'),
  'audio': tr('Sound problem'),
  'subtitles': tr('Subtitle problem'),
  'metadata': tr('Wrong details or poster'),
  'other': tr('Something else'),
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
    if (context.mounted) ScaffoldMessenger.of(context).showSnackBar(SnackBar(content: Text(errorText(e))));
    return;
  }
  if (!context.mounted) return;
  String what(Map x) => x['type'] == 'episode' ? 'S${x['parentIndex'] ?? 0}E${x['index'] ?? 0} · ${x['title']}' : '${x['title']}';
  await showDialog(
    context: context,
    builder: (ctx) => AlertDialog(
      title: Text(tr('History: {item}', {'item': item['title']})),
      content: SizedBox(
        width: 460,
        child: rows.isEmpty
            ? Text(tr('Not watched yet.'))
            : ListView(shrinkWrap: true, children: [
                for (final e in rows)
                  ListTile(
                    dense: true,
                    contentPadding: EdgeInsets.zero,
                    title: Text('${_when(e['at'])}${Poster.admin && e['user'] != null ? ' · ${e['user']}' : ''}'),
                    subtitle: Text([if (!playable && e['item'] is Map) what(e['item']), e['manual'] == true ? tr('marked as watched') : tr('watched')].join(' · ')),
                  ),
              ]),
      ),
      actions: [TextButton(onPressed: () => Navigator.pop(ctx), child: Text(tr('Close')))],
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
        title: Text(tr('Report a problem')),
        content: SizedBox(
          width: 420,
          child: Column(mainAxisSize: MainAxisSize.min, crossAxisAlignment: CrossAxisAlignment.start, children: [
            Text(tr('The owner of this server sees your report.'), style: TextStyle(color: context.faint)),
            const SizedBox(height: 8),
            DropdownButton<String>(
              value: kind,
              isExpanded: true,
              onChanged: (v) => set(() => kind = v ?? 'playback'),
              items: [for (final e in _problems.entries) DropdownMenuItem(value: e.key, child: Text(e.value))],
            ),
            TextField(controller: note, minLines: 3, maxLines: 6, decoration: InputDecoration(labelText: tr('What happens? (optional)'))),
          ]),
        ),
        actions: [
          TextButton(onPressed: () => Navigator.pop(ctx, false), child: Text(tr('Cancel'))),
          FilledButton(onPressed: () => Navigator.pop(ctx, true), child: Text(tr('Send'))),
        ],
      ),
    ),
  );
  final text = note.text;
  note.dispose();
  if (send != true) return;
  try {
    await api.post('/library/metadata/$id/report', {'kind': kind, 'note': text});
    if (context.mounted) ScaffoldMessenger.of(context).showSnackBar(SnackBar(content: Text(tr('Thanks – the problem was reported'))));
  } catch (e) {
    if (context.mounted) ScaffoldMessenger.of(context).showSnackBar(SnackBar(content: Text(errorText(e))));
  }
}

/// Admin: where the end credits start; the video counts as watched from there.
Future<void> creditsDialog(BuildContext context, Api api, Map<String, dynamic> item) async {
  final id = '${item['ratingKey'] ?? item['id']}';
  Map r;
  try {
    r = await api.get('/library/metadata/$id/markers');
  } catch (e) {
    if (context.mounted) ScaffoldMessenger.of(context).showSnackBar(SnackBar(content: Text(errorText(e))));
    return;
  }
  if (!context.mounted) return;
  final credits = (r['credits'] as List?) ?? const [];
  final inp = TextEditingController(text: credits.isEmpty ? '' : fmtClock(credits.first as num));
  final sources = {
    'manual': tr('set by hand'),
    'chapters': tr('from a chapter “Credits” in the file'),
    'picture': tr('found in the picture (a dark screen with text at the end)'),
    'none': tr('none found'),
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
      if (ctx.mounted) ScaffoldMessenger.of(ctx).showSnackBar(SnackBar(content: Text(errorText(e))));
    }
  }

  await showDialog(
    context: context,
    builder: (ctx) => AlertDialog(
      title: Text(tr('Credits')),
      content: SizedBox(
        width: 420,
        child: Column(mainAxisSize: MainAxisSize.min, crossAxisAlignment: CrossAxisAlignment.start, children: [
          Text(tr('Credits marker: {sources}. The video counts as watched from {fmtClock} of {fmtClock2}.', {'sources': sources['${r['source']}'] ?? r['source'], 'fmtClock': fmtClock((r['watchedAt'] ?? 0) as num), 'fmtClock2': fmtClock((r['duration'] ?? 0) as num)}), style: TextStyle(color: context.faint)),
          const SizedBox(height: 8),
          TextField(controller: inp, decoration: InputDecoration(labelText: tr('Credits start at (h:min:s)'), hintText: 'z. B. 1:52:30')),
        ]),
      ),
      actions: [
        if (r['source'] == 'manual') TextButton(onPressed: () => save(ctx, null), child: Text(tr("Use the file's chapters"))),
        TextButton(
          onPressed: () async {
            try {
              await api.post('/library/metadata/$id/credits');
              if (ctx.mounted) {
                Navigator.pop(ctx);
                ScaffoldMessenger.of(ctx).showSnackBar(SnackBar(content: Text(tr('Detecting credits…'))));
              }
            } catch (e) {
              if (ctx.mounted) ScaffoldMessenger.of(ctx).showSnackBar(SnackBar(content: Text(errorText(e))));
            }
          },
          child: Text(tr('Detect again')),
        ),
        TextButton(onPressed: () => Navigator.pop(ctx), child: Text(tr('Cancel'))),
        FilledButton(
          onPressed: () {
            final ms = parse(inp.text);
            if (ms == null) {
              ScaffoldMessenger.of(ctx).showSnackBar(SnackBar(content: Text(tr('Enter a time like 1:52:30'))));
              return;
            }
            save(ctx, [ms]);
          },
          child: Text(tr('Save')),
        ),
      ],
    ),
  );
  inp.dispose();
}
