import 'package:flutter/material.dart';
import 'package:shared_preferences/shared_preferences.dart';
import '../api.dart';
import '../screens/home.dart' show openItem;
import '../screens/player.dart';
import '../theme.dart';
import 'bar.dart';
import 'poster.dart';
import 'states.dart';

/// "1 h 5 min", "25 min", like the web app.
String minutesText(num ms) {
  final m = (ms / 60000).round();
  if (m < 1) return '${(ms / 1000).round().clamp(1, 59)} s';
  return m >= 60 ? '${m ~/ 60} h ${m % 60} min' : '$m min';
}

Future<void> _playNext(BuildContext context, Api api, Map<String, dynamic> it) async {
  Map<String, dynamic> e = it;
  if (it['type'] == 'show' || it['type'] == 'season') {
    final List<Map<String, dynamic>> leaves;
    try {
      leaves = await api.allLeaves('${it['ratingKey']}');
    } catch (err) {
      if (context.mounted) ScaffoldMessenger.of(context).showSnackBar(SnackBar(content: Text(errorText(err))));
      return;
    }
    if (leaves.isEmpty) return;
    e = leaves.firstWhere((x) => ((x['viewCount'] ?? 0) as num) == 0, orElse: () => leaves.first);
  }
  if (context.mounted) await Navigator.push(context, MaterialPageRoute(builder: (_) => PlayerScreen(api: api, item: e)));
}

/// The dice of a library: the title picked at random, with its description, to play, to open, or to throw again.
Future<void> randomDialog(BuildContext context, Api api, Map<String, dynamic> it, VoidCallback again) {
  const playable = {'movie', 'episode', 'track', 'clip'};
  final type = '${it['type']}';
  final name = type == 'episode' ? '${it['grandparentTitle']} · ${it['title']}' : type == 'season' ? '${it['parentTitle']} · ${it['title']}' : '${it['title']}';
  final dur = ((it['duration'] ?? 0) as num), leaves = ((it['leafCount'] ?? 0) as num).toInt();
  final off = ((it['viewOffset'] ?? 0) as num);
  final genres = [for (final g in (it['genres'] as List? ?? const [])) '$g'].take(3);
  final facts = [
    if (it['year'] != null) '${it['year']}',
    if (type == 'show') '$leaves Folgen' else if (dur > 0) minutesText(dur),
    ...genres,
  ].join(' · ');
  final canPlay = it['missing'] != true && (playable.contains(type) || ((type == 'show' || type == 'season') && leaves > 0));
  return showDialog<void>(
    context: context,
    builder: (ctx) {
      final narrow = isNarrow(ctx);
      final text = Column(crossAxisAlignment: narrow ? CrossAxisAlignment.center : CrossAxisAlignment.start, mainAxisSize: MainAxisSize.min, children: [
        Text(name, style: Theme.of(ctx).textTheme.titleMedium, textAlign: narrow ? TextAlign.center : TextAlign.start),
        if (facts.isNotEmpty) Padding(padding: const EdgeInsets.only(top: 4), child: Text(facts, style: TextStyle(color: ctx.dim))),
        if ('${it['summary'] ?? ''}'.isNotEmpty) Padding(padding: const EdgeInsets.only(top: 10), child: Text('${it['summary']}', maxLines: 7, overflow: TextOverflow.ellipsis)),
      ]);
      void open() {
        Navigator.pop(ctx);
        openItem(context, api, it);
      }

      final poster = Poster(api: api, item: it, width: 130, onTap: open);
      return AlertDialog(
        title: const Text('Zufällig ausgewählt'),
        content: SizedBox(
          width: 520,
          child: SingleChildScrollView(
            child: narrow
                ? Column(mainAxisSize: MainAxisSize.min, children: [poster, const SizedBox(height: 12), text])
                : Row(crossAxisAlignment: CrossAxisAlignment.start, children: [poster, const SizedBox(width: 18), Expanded(child: text)]),
          ),
        ),
        actions: [
          TextButton(onPressed: () => Navigator.pop(ctx), child: const Text('Schließen')),
          TextButton(
            onPressed: () {
              Navigator.pop(ctx);
              again();
            },
            child: const Text('🎲 Noch einmal'),
          ),
          TextButton(onPressed: open, child: const Text('Mehr Infos')),
          if (canPlay)
            FilledButton.icon(
              onPressed: () {
                Navigator.pop(ctx);
                _playNext(context, api, it);
              },
              icon: const Icon(Icons.play_arrow),
              label: Text(type == 'show' || type == 'season' ? 'Nächste Folge' : off >= 1000 ? 'Fortsetzen' : 'Abspielen'),
            ),
        ],
      );
    },
  );
}

const shortTimes = [20, 30, 45, 60, 90];

/// Under a poster of "Etwas Kurzes": how long it takes (an episode keeps its number).
String shortCaption(Map<String, dynamic> it) {
  final dur = (it['duration'] ?? 0) as num, off = (it['viewOffset'] ?? 0) as num;
  final time = off > 0 ? 'noch ${minutesText(dur - off)}' : minutesText(dur);
  return it['type'] == 'episode' ? 'S${it['parentIndex'] ?? '?'}E${it['index'] ?? '?'} · $time' : time;
}

/// "Etwas Kurzes": how much time there is, and what fits into it from what you are watching (a title begun
/// counts with the time left) and from your watchlist.
Future<void> shortDialog(BuildContext context, Api api) async {
  final prefs = await SharedPreferences.getInstance();
  var minutes = prefs.getInt('shortMinutes') ?? 30;
  var data = api.short(minutes);
  final own = TextEditingController(text: shortTimes.contains(minutes) ? '' : '$minutes');
  if (!context.mounted) return;
  await showDialog<void>(
    context: context,
    builder: (ctx) => StatefulBuilder(builder: (ctx, set) {
      void choose(int m) {
        m = m.clamp(1, 600);
        prefs.setInt('shortMinutes', m);
        set(() {
          minutes = m;
          data = api.short(m);
          if (shortTimes.contains(m)) own.clear();
        });
      }

      return AlertDialog(
        title: const Text('Etwas Kurzes'),
        content: SizedBox(
          width: 640,
          child: Column(mainAxisSize: MainAxisSize.min, crossAxisAlignment: CrossAxisAlignment.start, children: [
            Text('Wie viel Zeit hast du? Was hineinpasst, kommt aus dem, was du gerade schaust, und von deiner Merkliste.', style: TextStyle(color: ctx.dim)),
            const SizedBox(height: 12),
            Wrap(spacing: 8, runSpacing: 8, crossAxisAlignment: WrapCrossAlignment.center, children: [
              for (final m in shortTimes) ChoiceChip(label: Text('$m min'), selected: m == minutes, onSelected: (_) => choose(m)),
              SizedBox(
                width: 90,
                child: TextField(
                  controller: own,
                  keyboardType: TextInputType.number,
                  decoration: const InputDecoration(hintText: 'eigene', suffixText: 'min', isDense: true),
                  onSubmitted: (v) {
                    final m = int.tryParse(v.trim());
                    if (m != null && m > 0) choose(m);
                  },
                ),
              ),
            ]),
            const SizedBox(height: 16),
            Flexible(
              child: FutureBuilder<List<Map<String, dynamic>>>(
                future: data,
                builder: (ctx, snap) {
                  if (snap.hasError) return Text(errorText(snap.error!));
                  if (!snap.hasData) return const Padding(padding: EdgeInsets.all(16), child: Center(child: CircularProgressIndicator()));
                  final items = snap.data!;
                  if (items.isEmpty) return Text('Nichts, was du gerade schaust oder auf deiner Merkliste hast, passt in $minutes Minuten.', style: TextStyle(color: ctx.dim));
                  return SingleChildScrollView(
                    child: Wrap(spacing: 12, runSpacing: 16, children: [
                      for (final it in items)
                        Poster(
                          api: api,
                          item: it,
                          width: 130,
                          caption: shortCaption(it),
                          onTap: () {
                            Navigator.pop(ctx);
                            openItem(context, api, it);
                          },
                        ),
                    ]),
                  );
                },
              ),
            ),
          ]),
        ),
        actions: [TextButton(onPressed: () => Navigator.pop(ctx), child: const Text('Schließen'))],
      );
    }),
  );
  own.dispose();
}
