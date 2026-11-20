import 'package:flutter/material.dart';
import '../api.dart';
import '../screens/player.dart';
import '../theme.dart';

String _title(Map<String, dynamic> x) {
  if (x['type'] == 'episode') return '${x['grandparentTitle'] ?? ''} · S${x['parentIndex'] ?? 0}E${x['index'] ?? 0} ${x['title']}';
  if (x['type'] == 'track') return '${x['grandparentTitle'] ?? ''} · ${x['title']}';
  return '${x['title']}';
}

/// The queue ("Zur Warteschlange"): shows it, moves entries up, removes them, plays it or clears it.
Future<void> showQueueDialog(BuildContext context, Api api) {
  final q = PlayerScreen.userQueue;
  return showDialog(
    context: context,
    builder: (ctx) => StatefulBuilder(
      builder: (ctx, set) => AlertDialog(
        title: Text('Warteschlange (${q.length})'),
        content: SizedBox(
          width: 460,
          child: q.isEmpty
              ? const Text('Die Warteschlange ist leer. Über das „…“-Menü eines Titels lässt sich etwas hinzufügen.')
              : Column(mainAxisSize: MainAxisSize.min, crossAxisAlignment: CrossAxisAlignment.start, children: [
                  Padding(padding: EdgeInsets.only(bottom: 8), child: Text('Läuft nacheinander, nach dem aktuellen Video.', style: TextStyle(color: context.faint))),
                  Flexible(
                    child: ListView(shrinkWrap: true, children: [
                      for (var i = 0; i < q.length; i++)
                        ListTile(
                          dense: true,
                          contentPadding: EdgeInsets.zero,
                          title: Text(_title(q[i]), maxLines: 1, overflow: TextOverflow.ellipsis),
                          trailing: Row(mainAxisSize: MainAxisSize.min, children: [
                            if (i > 0) IconButton(tooltip: 'Nach oben', icon: const Icon(Icons.arrow_upward, size: 18), onPressed: () => set(() => q.insert(i - 1, q.removeAt(i)))),
                            IconButton(tooltip: 'Entfernen', icon: const Icon(Icons.close, size: 18), onPressed: () => set(() => q.removeAt(i))),
                          ]),
                        ),
                    ]),
                  ),
                ]),
        ),
        actions: [
          if (q.isNotEmpty) TextButton(onPressed: () => set(q.clear), child: const Text('Leeren')),
          TextButton(onPressed: () => Navigator.pop(ctx), child: const Text('Schließen')),
          if (q.isNotEmpty)
            FilledButton(
              onPressed: () {
                final first = q.removeAt(0);
                Navigator.pop(ctx);
                Navigator.push(context, MaterialPageRoute(builder: (_) => PlayerScreen(api: api, item: first)));
              },
              child: const Text('Abspielen'),
            ),
        ],
      ),
    ),
  );
}
