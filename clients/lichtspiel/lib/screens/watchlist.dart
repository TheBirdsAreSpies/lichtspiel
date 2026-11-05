import 'package:flutter/material.dart';
import '../api.dart';
import '../widgets/poster.dart';
import 'detail.dart';
import 'library.dart' show FilterDialog, filterName, valueName;

/// What you want to watch later, with the type, sort, year and title filters of the web app.
class WatchlistScreen extends StatefulWidget {
  final Api api;
  const WatchlistScreen({super.key, required this.api});
  @override
  State<WatchlistScreen> createState() => _WatchlistScreenState();
}

class _WatchlistScreenState extends State<WatchlistScreen> {
  static const sorts = {
    '': 'Zur Merkliste hinzugefügt (neueste)',
    'oldest': 'Zur Merkliste hinzugefügt (älteste)',
    'title': 'Titel',
    'year': 'Jahr',
    'rating': 'Bewertung',
    'added_at': 'Zur Bibliothek hinzugefügt',
  };
  String type = '', sort = '';
  final filters = <String, String>{};
  List<Map<String, dynamic>>? available; // the filters with the values found on the watchlist (per type)
  final search = TextEditingController(), from = TextEditingController(), to = TextEditingController();
  List<Map<String, dynamic>>? items;
  String? error;
  int gen = 0;

  @override
  void initState() {
    super.initState();
    reload();
  }

  Future<void> reload() async {
    final my = ++gen;
    setState(() => error = null);
    try {
      final r = await widget.api.watchlist({
        if (type.isNotEmpty) 'type': type,
        if (sort.isNotEmpty) 'sort': sort,
        if (from.text.trim().isNotEmpty) 'year>': from.text.trim(),
        if (to.text.trim().isNotEmpty) 'year<': to.text.trim(),
        if (search.text.trim().isNotEmpty) 'q': search.text.trim(),
        ...filters,
      });
      if (mounted && my == gen) setState(() => items = r);
    } catch (e) {
      if (mounted && my == gen) setState(() => error = '$e');
    }
  }

  Future<void> openFilters() async {
    try {
      available ??= await widget.api.watchlistFilters(type: type);
    } catch (e) {
      if (mounted) ScaffoldMessenger.of(context).showSnackBar(SnackBar(content: Text('$e')));
      return;
    }
    if (!mounted) return;
    await showDialog(context: context, builder: (_) => FilterDialog(available: available!, filters: filters, onChanged: reload));
    if (mounted) setState(() {});
  }

  Widget field(TextEditingController c, String hint, double w, {bool number = false}) => SizedBox(
        width: w,
        child: TextField(
          controller: c,
          keyboardType: number ? TextInputType.number : TextInputType.text,
          decoration: InputDecoration(isDense: true, hintText: hint, border: const OutlineInputBorder()),
          onSubmitted: (_) => reload(),
        ),
      );

  @override
  Widget build(BuildContext context) {
    final list = items;
    return Scaffold(
      appBar: AppBar(title: const Text('Merkliste')),
      body: Column(children: [
        Padding(
          padding: const EdgeInsets.fromLTRB(16, 8, 16, 0),
          child: Wrap(spacing: 8, runSpacing: 8, crossAxisAlignment: WrapCrossAlignment.center, children: [
            SegmentedButton<String>(
              showSelectedIcon: false,
              segments: const [
                ButtonSegment(value: '', label: Text('Alle')),
                ButtonSegment(value: 'movie', label: Text('Filme')),
                ButtonSegment(value: 'show', label: Text('Serien')),
                ButtonSegment(value: 'episode', label: Text('Folgen')),
              ],
              selected: {type},
              onSelectionChanged: (s) {
                type = s.first;
                filters.clear(); // the kinds of filter differ per type
                available = null;
                reload();
              },
            ),
            PopupMenuButton<String>(
              tooltip: 'Sortieren',
              onSelected: (v) {
                sort = v;
                reload();
              },
              itemBuilder: (_) => [for (final e in sorts.entries) CheckedPopupMenuItem(value: e.key, checked: e.key == sort, child: Text(e.value))],
              child: Chip(avatar: const Icon(Icons.sort, size: 18), label: Text(sorts[sort]!)),
            ),
            OutlinedButton.icon(onPressed: openFilters, icon: const Icon(Icons.filter_list), label: Text(filters.isNotEmpty ? 'Filter (${filters.length})' : 'Filter')),
            field(search, 'Nach Titel filtern', 170),
            field(from, 'Jahr ab', 90, number: true),
            field(to, 'Jahr bis', 90, number: true),
            if (list != null) Text(list.length == 1 ? '1 Eintrag' : '${list.length} Einträge', style: const TextStyle(color: Colors.white60)),
          ]),
        ),
        if (filters.isNotEmpty)
          Padding(
            padding: const EdgeInsets.fromLTRB(16, 8, 16, 0),
            child: Align(
              alignment: Alignment.centerLeft,
              child: Wrap(spacing: 8, runSpacing: 4, children: [
                for (final e in filters.entries)
                  InputChip(
                    label: Text(e.value == 'true' ? filterName(e.key) : e.value == 'false' ? '${filterName(e.key)}: nein' : '${filterName(e.key)}: ${valueName(e.key, e.value)}'),
                    onDeleted: () {
                      filters.remove(e.key);
                      reload();
                    },
                  ),
                if (filters.length > 1)
                  TextButton(
                    onPressed: () {
                      filters.clear();
                      reload();
                    },
                    child: const Text('Filter löschen'),
                  ),
              ]),
            ),
          ),
        Expanded(
          child: error != null
              ? Center(child: Text(error!))
              : list == null
                  ? const Center(child: CircularProgressIndicator())
                  : list.isEmpty
                      ? const Center(child: Text('Deine Merkliste ist leer. Nutze „Auf die Merkliste“ bei einem Film oder einer Serie.'))
                      : GridView.builder(
                          padding: const EdgeInsets.all(16),
                          gridDelegate: const SliverGridDelegateWithMaxCrossAxisExtent(maxCrossAxisExtent: 170, childAspectRatio: 0.48, crossAxisSpacing: 12, mainAxisSpacing: 12),
                          itemCount: list.length,
                          itemBuilder: (_, i) => Poster(
                            api: widget.api,
                            item: list[i],
                            width: 170,
                            onTap: () async {
                              await Navigator.push(context, MaterialPageRoute(builder: (_) => DetailScreen(api: widget.api, id: '${list[i]['ratingKey']}')));
                              reload();
                            },
                          ),
                        ),
        ),
      ]),
    );
  }
}
