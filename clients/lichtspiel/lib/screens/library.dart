import 'package:flutter/material.dart';
import '../api.dart';
import '../widgets/poster.dart';
import 'home.dart';

class LibraryScreen extends StatefulWidget {
  final Api api;
  final Map<String, dynamic> section;
  const LibraryScreen({super.key, required this.api, required this.section});
  @override
  State<LibraryScreen> createState() => _LibraryScreenState();
}

class _LibraryScreenState extends State<LibraryScreen> {
  static const sorts = {
    'title': 'Titel',
    'added_at:desc': 'Zuletzt hinzugefügt',
    'year:desc': 'Jahr (neu)',
    'rating:desc': 'Bewertung',
    'random': 'Zufällig',
  };
  static const page = 60;
  final items = <Map<String, dynamic>>[];
  final scroll = ScrollController();
  String sort = 'title';
  bool loading = false, done = false;
  String? error;

  @override
  void initState() {
    super.initState();
    scroll.addListener(() {
      if (scroll.position.pixels > scroll.position.maxScrollExtent - 600) load();
    });
    load();
  }

  @override
  void dispose() {
    scroll.dispose();
    super.dispose();
  }

  Future<void> load() async {
    if (loading || done) return;
    setState(() => loading = true);
    try {
      final more = await widget.api.items('${widget.section['key']}', sort: sort, offset: items.length, limit: page);
      items.addAll(more);
      if (more.length < page) done = true;
    } catch (e) {
      error = '$e';
    }
    if (mounted) setState(() => loading = false);
  }

  void resort(String s) {
    items.clear();
    done = false;
    error = null;
    setState(() => sort = s);
    load();
  }

  @override
  Widget build(BuildContext context) {
    return Scaffold(
      appBar: AppBar(
        title: Text('${widget.section['title']}'),
        actions: [
          PopupMenuButton<String>(
            icon: const Icon(Icons.sort),
            tooltip: 'Sortieren',
            onSelected: resort,
            itemBuilder: (_) => [for (final e in sorts.entries) CheckedPopupMenuItem(value: e.key, checked: e.key == sort, child: Text(e.value))],
          ),
        ],
      ),
      body: error != null && items.isEmpty
          ? Center(child: Text(error!))
          : GridView.builder(
              controller: scroll,
              padding: const EdgeInsets.all(16),
              gridDelegate: const SliverGridDelegateWithMaxCrossAxisExtent(maxCrossAxisExtent: 170, childAspectRatio: 0.48, crossAxisSpacing: 12, mainAxisSpacing: 12),
              itemCount: items.length + (loading ? 1 : 0),
              itemBuilder: (_, i) => i >= items.length
                  ? const Center(child: CircularProgressIndicator())
                  : Poster(api: widget.api, item: items[i], width: 170, onTap: () => openItem(context, widget.api, items[i])),
            ),
    );
  }
}
