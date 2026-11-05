import 'package:flutter/material.dart';
import '../api.dart';
import '../widgets/poster.dart';

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
            const Text('Der Besitzer dieses Servers sieht deine Meldung.', style: TextStyle(color: Colors.white54)),
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
