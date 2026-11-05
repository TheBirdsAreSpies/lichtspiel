import 'package:flutter/material.dart';
import 'package:flutter/services.dart';
import '../api.dart';
import '../screens/player.dart';

/// "Zusammen schauen": opens a room for [item] and shows the link/code to send; "Starten" joins it.
Future<void> watchTogether(BuildContext context, Api api, Map<String, dynamic> item) async {
  final id = '${item['ratingKey'] ?? item['id']}';
  try {
    final r = await api.post('/api/together', {'item': id, 'position': item['viewOffset'] ?? 0});
    final code = '${r['code']}';
    final link = '${api.base}/#/together/$code';
    if (!context.mounted) return;
    await showDialog(
      context: context,
      builder: (ctx) => AlertDialog(
        title: const Text('Zusammen schauen'),
        content: Column(mainAxisSize: MainAxisSize.min, crossAxisAlignment: CrossAxisAlignment.start, children: [
          const Text('Schick diesen Link oder Code an die, mit denen du schauen willst (sie brauchen ein Konto hier, das diese Mediathek sehen darf). Wer abspielt, pausiert oder springt, bewegt alle.', style: TextStyle(color: Colors.white54)),
          const SizedBox(height: 12),
          SelectableText(link),
          Text('Code: $code'),
        ]),
        actions: [
          TextButton(onPressed: () => Clipboard.setData(ClipboardData(text: link)), child: const Text('Link kopieren')),
          TextButton(onPressed: () => Navigator.pop(ctx), child: const Text('Schließen')),
          FilledButton(
            onPressed: () {
              Navigator.pop(ctx);
              joinTogetherWith(context, api, code);
            },
            child: const Text('Starten'),
          ),
        ],
      ),
    );
  } catch (e) {
    if (context.mounted) ScaffoldMessenger.of(context).showSnackBar(SnackBar(content: Text('$e')));
  }
}

/// Opens the player in the room [code]; the player takes video, place and pause state from the server.
Future<void> joinTogetherWith(BuildContext context, Api api, String code) async {
  try {
    final st = await api.get('/api/together/$code');
    if (!context.mounted) return;
    await Navigator.push(context, MaterialPageRoute(builder: (_) => PlayerScreen(api: api, item: {'id': st['item'], 'ratingKey': '${st['item']}', 'viewOffset': st['position']}, together: code)));
  } catch (e) {
    if (context.mounted) ScaffoldMessenger.of(context).showSnackBar(SnackBar(content: Text('$e')));
  }
}

/// Asks for a room's link or code and joins it.
Future<void> joinTogetherDialog(BuildContext context, Api api) async {
  final c = TextEditingController();
  final text = await showDialog<String>(
    context: context,
    builder: (ctx) => AlertDialog(
      title: const Text('Einem Raum beitreten'),
      content: TextField(controller: c, autofocus: true, decoration: const InputDecoration(labelText: 'Link oder Code'), onSubmitted: (v) => Navigator.pop(ctx, v)),
      actions: [
        TextButton(onPressed: () => Navigator.pop(ctx), child: const Text('Abbrechen')),
        FilledButton(onPressed: () => Navigator.pop(ctx, c.text), child: const Text('Beitreten')),
      ],
    ),
  );
  c.dispose();
  final code = (text ?? '').trim().split('/').last.trim();
  if (code.isEmpty || !context.mounted) return;
  await joinTogetherWith(context, api, code);
}
