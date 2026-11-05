import 'package:flutter/material.dart';
import '../api.dart';
import '../widgets/poster.dart';
import 'home.dart';

/// An actor and everything they are in (the web app's person page).
class PersonScreen extends StatefulWidget {
  final Api api;
  final String keyId; // the person's key, or "byname"
  final String name;
  const PersonScreen({super.key, required this.api, required this.keyId, required this.name});
  @override
  State<PersonScreen> createState() => _PersonScreenState();
}

class _PersonScreenState extends State<PersonScreen> {
  late final Future<Map<String, dynamic>> data = widget.api.person(widget.keyId, widget.name);
  late Future<Map<String, dynamic>> details = widget.api.personDetails(widget.keyId, widget.name);

  static String day(String? iso) {
    final d = DateTime.tryParse(iso ?? '');
    return d == null ? '' : '${d.day}.${d.month}.${d.year}';
  }

  /// Biography, birth, photos and filmography from TMDB, below the library part.
  Widget more(Map<String, dynamic> r) {
    return FutureBuilder<Map<String, dynamic>>(
      future: details,
      builder: (context, snap) {
        if (snap.hasError) return Padding(padding: const EdgeInsets.only(top: 24), child: Text('${snap.error}', style: const TextStyle(color: Colors.white54)));
        if (!snap.hasData) return const Padding(padding: EdgeInsets.only(top: 24), child: Center(child: CircularProgressIndicator()));
        final d = snap.data!;
        final photos = ((d['photos'] as List?) ?? const []).map((e) => Map<String, dynamic>.from(e)).toList();
        final credits = ((d['credits'] as List?) ?? const []).map((e) => Map<String, dynamic>.from(e)).toList();
        final mine = credits.where((c) => c['item'] != null).toList();
        final others = credits.where((c) => c['item'] == null).toList();
        final born = d['birthday'] != null ? 'Geboren am ${day(d['birthday'])}' : '';
        final died = d['deathday'] != null ? 'Gestorben am ${day(d['deathday'])}' : '';
        final facts = [born, died, '${d['place_of_birth'] ?? ''}'].where((x) => x.isNotEmpty).join(' · ');
        Widget head(String t) => Padding(padding: const EdgeInsets.only(top: 28, bottom: 10), child: Text(t, style: Theme.of(context).textTheme.titleLarge));
        return Column(crossAxisAlignment: CrossAxisAlignment.start, children: [
          if (facts.isNotEmpty) Padding(padding: const EdgeInsets.only(top: 20), child: Text(facts, style: const TextStyle(color: Colors.white60))),
          if ('${d['biography'] ?? ''}'.isNotEmpty) Padding(padding: const EdgeInsets.only(top: 10), child: Text('${d['biography']}')),
          if (photos.length > 1) ...[
            head('Fotos'),
            SizedBox(
              height: 200,
              child: ListView(scrollDirection: Axis.horizontal, children: [
                for (final p in photos)
                  Padding(
                    padding: const EdgeInsets.only(right: 10),
                    child: InkWell(
                      onTap: () => showDialog(context: context, builder: (_) => Dialog(child: InteractiveViewer(child: Image.network('${p['full']}')))),
                      child: ClipRRect(borderRadius: BorderRadius.circular(6), child: Image.network('${p['url']}', height: 200, fit: BoxFit.cover)),
                    ),
                  ),
              ]),
            ),
          ],
          if (credits.isNotEmpty) ...[
            head('Filmografie (${mine.length} von ${credits.length} in deinen Bibliotheken)'),
            Wrap(spacing: 12, runSpacing: 16, children: [
              for (final c in [...mine, ...others])
                SizedBox(
                  width: 140,
                  child: c['item'] != null
                      ? Column(crossAxisAlignment: CrossAxisAlignment.start, children: [
                          Poster(api: widget.api, item: Map<String, dynamic>.from(c['item']), onTap: () => openItem(context, widget.api, Map<String, dynamic>.from(c['item']))),
                          if ('${c['role'] ?? ''}'.isNotEmpty) Text('${c['role']}', maxLines: 1, overflow: TextOverflow.ellipsis, style: const TextStyle(fontSize: 12, color: Colors.white54)),
                        ])
                      : Opacity(
                          opacity: .55,
                          child: Tooltip(
                            message: 'Nicht in deinen Bibliotheken',
                            child: Column(crossAxisAlignment: CrossAxisAlignment.start, children: [
                              AspectRatio(
                                aspectRatio: 2 / 3,
                                child: ClipRRect(
                                  borderRadius: BorderRadius.circular(6),
                                  child: c['poster'] != null ? Image.network('${c['poster']}', fit: BoxFit.cover, errorBuilder: (_, __, ___) => Container(color: Colors.white10)) : Container(color: Colors.white10),
                                ),
                              ),
                              const SizedBox(height: 6),
                              Text('${c['title']}', maxLines: 1, overflow: TextOverflow.ellipsis, style: const TextStyle(fontWeight: FontWeight.w500)),
                              Text([c['year'], c['role']].where((x) => x != null && '$x'.isNotEmpty).join(' · '), maxLines: 1, overflow: TextOverflow.ellipsis, style: const TextStyle(fontSize: 12, color: Colors.white54)),
                            ]),
                          ),
                        ),
                ),
            ]),
          ],
          Padding(
            padding: const EdgeInsets.only(top: 20),
            child: Align(alignment: Alignment.centerLeft, child: TextButton(onPressed: () => setState(() => details = widget.api.personDetails(widget.keyId, widget.name, refresh: true)), child: const Text('Details neu laden'))),
          ),
        ]);
      },
    );
  }

  @override
  Widget build(BuildContext context) {
    return Scaffold(
      appBar: AppBar(title: Text(widget.name)),
      body: FutureBuilder<Map<String, dynamic>>(
        future: data,
        builder: (context, snap) {
          if (snap.hasError) return Center(child: Text('${snap.error}'));
          if (!snap.hasData) return const Center(child: CircularProgressIndicator());
          final r = snap.data!;
          final items = ((r['Metadata'] as List?) ?? const []).map((e) => Map<String, dynamic>.from(e)).toList();
          final name = '${r['name'] ?? widget.name}';
          final src = r['thumb'] != null ? widget.api.img('${r['thumb']}') : '${r['photo'] ?? ''}';
          final initials = name.split(' ').where((w) => w.isNotEmpty).map((w) => w[0]).take(2).join();
          final n = (r['size'] ?? items.length) as int;
          return ListView(padding: const EdgeInsets.all(20), children: [
            Row(crossAxisAlignment: CrossAxisAlignment.start, children: [
              ClipRRect(
                borderRadius: BorderRadius.circular(8),
                child: SizedBox(
                  width: 160,
                  height: 220,
                  child: src.isEmpty
                      ? Container(color: Colors.white10, alignment: Alignment.center, child: Text(initials, style: const TextStyle(fontSize: 42)))
                      : Image.network(src, fit: BoxFit.cover, errorBuilder: (_, __, ___) => Container(color: Colors.white10, alignment: Alignment.center, child: Text(initials, style: const TextStyle(fontSize: 42)))),
                ),
              ),
              const SizedBox(width: 20),
              Expanded(
                child: Column(crossAxisAlignment: CrossAxisAlignment.start, children: [
                  Text(name, style: Theme.of(context).textTheme.headlineMedium),
                  const SizedBox(height: 6),
                  Text(n == 1 ? 'In 1 Film oder Serie deiner Bibliotheken' : 'In $n Filmen und Serien deiner Bibliotheken', style: const TextStyle(color: Colors.white60)),
                ]),
              ),
            ]),
            const SizedBox(height: 24),
            if (items.isEmpty)
              const Center(child: Text('Nichts in deinen Bibliotheken.'))
            else
              Wrap(spacing: 12, runSpacing: 16, children: [
                for (final it in items)
                  SizedBox(
                    width: 140,
                    child: Column(crossAxisAlignment: CrossAxisAlignment.start, children: [
                      Poster(api: widget.api, item: it, onTap: () => openItem(context, widget.api, it)),
                      if (it['role'] != null) Text('als ${it['role']}', maxLines: 1, overflow: TextOverflow.ellipsis, style: const TextStyle(fontSize: 12, color: Colors.white54)),
                    ]),
                  ),
              ]),
            more(r),
          ]);
        },
      ),
    );
  }
}
