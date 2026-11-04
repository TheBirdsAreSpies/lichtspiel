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
          ]);
        },
      ),
    );
  }
}
