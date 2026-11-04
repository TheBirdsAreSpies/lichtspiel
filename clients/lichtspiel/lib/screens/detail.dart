import 'package:flutter/material.dart';
import '../api.dart';
import '../widgets/poster.dart';
import 'home.dart';
import 'player.dart';

String fmtMs(int ms) {
  final m = ms ~/ 60000;
  return m >= 60 ? '${m ~/ 60} h ${m % 60} min' : '$m min';
}

/// Movie, show, season or episode. Shows and seasons list their children; playable items get a play button.
class DetailScreen extends StatefulWidget {
  final Api api;
  final String id;
  const DetailScreen({super.key, required this.api, required this.id});
  @override
  State<DetailScreen> createState() => _DetailScreenState();
}

class _DetailScreenState extends State<DetailScreen> {
  late Future<Map<String, dynamic>> item = widget.api.item(widget.id);
  List<Map<String, dynamic>> kids = [];

  @override
  void initState() {
    super.initState();
    item.then((it) async {
      if (it['type'] == 'show' || it['type'] == 'season') {
        final k = await widget.api.children(widget.id);
        if (mounted) setState(() => kids = k);
      }
    });
  }

  void play(Map<String, dynamic> it) async {
    await Navigator.push(context, MaterialPageRoute(builder: (_) => PlayerScreen(api: widget.api, item: it)));
    if (mounted) setState(() => item = widget.api.item(widget.id)); // refresh resume position
  }

  @override
  Widget build(BuildContext context) {
    final api = widget.api;
    return Scaffold(
      appBar: AppBar(),
      body: FutureBuilder<Map<String, dynamic>>(
        future: item,
        builder: (context, snap) {
          if (snap.hasError) return Center(child: Text('${snap.error}'));
          if (!snap.hasData) return const Center(child: CircularProgressIndicator());
          final it = snap.data!;
          final playable = it['type'] == 'movie' || it['type'] == 'episode';
          final off = (it['viewOffset'] ?? 0) as int;
          final meta = [
            if (it['year'] != null) '${it['year']}',
            if ((it['duration'] ?? 0) > 0) fmtMs(it['duration']),
            if (it['contentRating'] != null) '${it['contentRating']}',
            if (it['rating'] != null) '★ ${it['rating']}',
          ].join('  ·  ');
          return ListView(padding: const EdgeInsets.all(20), children: [
            Row(crossAxisAlignment: CrossAxisAlignment.start, children: [
              SizedBox(width: 180, child: AspectRatio(aspectRatio: 2 / 3, child: ClipRRect(borderRadius: BorderRadius.circular(8), child: Image.network(api.img(it['thumb']), fit: BoxFit.cover, errorBuilder: (_, __, ___) => Container(color: Colors.white10))))),
              const SizedBox(width: 20),
              Expanded(
                child: Column(crossAxisAlignment: CrossAxisAlignment.start, children: [
                  if (it['grandparentTitle'] != null) Text('${it['grandparentTitle']} · ${it['parentTitle'] ?? ''}', style: const TextStyle(color: Colors.white60)),
                  Text('${it['title']}', style: Theme.of(context).textTheme.headlineMedium),
                  const SizedBox(height: 6),
                  Text(meta, style: const TextStyle(color: Colors.white60)),
                  if ((it['genres'] as List?)?.isNotEmpty ?? false) Padding(padding: const EdgeInsets.only(top: 4), child: Text((it['genres'] as List).join(', '), style: const TextStyle(color: Colors.white60))),
                  const SizedBox(height: 16),
                  if (playable)
                    Wrap(spacing: 10, children: [
                      FilledButton.icon(onPressed: () => play(it), icon: const Icon(Icons.play_arrow), label: Text(off > 0 ? 'Fortsetzen (${fmtMs(off)})' : 'Abspielen')),
                      if (off > 0) OutlinedButton(onPressed: () => play({...it, 'viewOffset': 0}), child: const Text('Von vorn')),
                    ]),
                  const SizedBox(height: 16),
                  Text('${it['summary'] ?? ''}'),
                ]),
              ),
            ]),
            if (kids.isNotEmpty) ...[
              const SizedBox(height: 24),
              Text(it['type'] == 'show' ? 'Staffeln' : 'Folgen', style: Theme.of(context).textTheme.titleMedium),
              const SizedBox(height: 8),
              if (it['type'] == 'show')
                Wrap(spacing: 12, runSpacing: 16, children: [for (final k in kids) Poster(api: api, item: k, onTap: () => openItem(context, api, k))])
              else
                for (final k in kids)
                  ListTile(
                    leading: Text('${k['index'] ?? ''}'),
                    title: Text('${k['title']}'),
                    subtitle: Text(fmtMs((k['duration'] ?? 0) as int)),
                    trailing: ((k['viewCount'] ?? 0) as int) > 0 ? const Icon(Icons.check_circle, color: Color(0xFFE5A00D)) : null,
                    onTap: () => openItem(context, api, k),
                  ),
            ],
          ]);
        },
      ),
    );
  }
}
