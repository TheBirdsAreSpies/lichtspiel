import 'package:flutter/material.dart';
import '../api.dart';
import '../widgets/poster.dart';
import 'detail.dart';
import 'library.dart' show showFilters, filterName, valueName;
import '../widgets/bar.dart';
import '../widgets/states.dart';
import '../theme.dart';

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
  Object? error;
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
      if (mounted && my == gen) setState(() => error = e);
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
    await showFilters(context, available: available!, filters: filters, onChanged: reload);
    if (mounted) setState(() {});
  }

  /// Phones: the year range, which has no room in the bar.
  Future<void> openYears() async {
    await showModalBottomSheet(
      context: context,
      isScrollControlled: true,
      useSafeArea: true,
      showDragHandle: true,
      builder: (ctx) => Padding(
        padding: EdgeInsets.fromLTRB(16, 0, 16, 16 + MediaQuery.viewInsetsOf(ctx).bottom),
        child: Column(mainAxisSize: MainAxisSize.min, crossAxisAlignment: CrossAxisAlignment.start, children: [
          Text('Jahr', style: Theme.of(ctx).textTheme.titleSmall),
          const SizedBox(height: 8),
          Row(children: [Expanded(child: field(from, 'ab', null, number: true)), const SizedBox(width: 12), Expanded(child: field(to, 'bis', null, number: true))]),
          const SizedBox(height: 16),
          Align(alignment: Alignment.centerRight, child: FilledButton(onPressed: () => Navigator.pop(ctx), child: const Text('Anwenden'))),
        ]),
      ),
    );
    if (mounted) reload();
  }

  Widget field(TextEditingController c, String hint, double? w, {bool number = false}) => SizedBox(
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
    final narrow = isNarrow(context);
    final years = (from.text.trim().isNotEmpty ? 1 : 0) + (to.text.trim().isNotEmpty ? 1 : 0);
    final types = SegmentedButton<String>(
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
    );
    final compact = Padding(
      padding: const EdgeInsets.fromLTRB(12, 8, 4, 0),
      child: Column(children: [
        SizedBox(width: double.infinity, child: types),
        const SizedBox(height: 8),
        Row(children: [
          Expanded(child: field(search, 'Nach Titel filtern', null)),
          IconButton(tooltip: 'Filter', onPressed: openFilters, icon: Badge(isLabelVisible: filters.isNotEmpty, label: Text('${filters.length}'), child: const Icon(Icons.filter_list))),
          PopupMenuButton<String>(
            tooltip: 'Sortieren: ${sorts[sort]}',
            icon: const Icon(Icons.sort),
            onSelected: (v) {
              sort = v;
              reload();
            },
            itemBuilder: (_) => [for (final e in sorts.entries) CheckedPopupMenuItem(value: e.key, checked: e.key == sort, child: Text(e.value))],
          ),
          IconButton(tooltip: 'Jahr', onPressed: openYears, icon: Badge(isLabelVisible: years > 0, label: Text('$years'), child: const Icon(Icons.date_range_outlined))),
        ]),
      ]),
    );
    return Scaffold(
      appBar: AppBar(
        title: narrow && list != null
            ? Column(crossAxisAlignment: CrossAxisAlignment.start, children: [
                const Text('Merkliste'),
                Text(list.length == 1 ? '1 Eintrag' : '${list.length} Einträge', style: TextStyle(fontSize: 12, color: context.dim)),
              ])
            : const Text('Merkliste'),
      ),
      body: Column(children: [
        if (narrow) compact else Padding(
          padding: const EdgeInsets.fromLTRB(16, 8, 16, 0),
          child: Wrap(spacing: 8, runSpacing: 8, crossAxisAlignment: WrapCrossAlignment.center, children: [
            types,
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
            if (list != null) Text(list.length == 1 ? '1 Eintrag' : '${list.length} Einträge', style: TextStyle(color: context.dim)),
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
              ? ErrorView(error: error!, onRetry: reload)
              : list == null
                  ? const GridSkeleton()
                  : list.isEmpty
                      ? ErrorView(
                          error: filters.isNotEmpty || years > 0 || search.text.trim().isNotEmpty || type.isNotEmpty
                              ? 'Nichts auf der Merkliste passt zu diesen Filtern.'
                              : 'Deine Merkliste ist leer. Nutze „Auf die Merkliste“ bei einem Film oder einer Serie.',
                        )
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
