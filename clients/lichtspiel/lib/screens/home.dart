import 'dart:async';
import 'package:flutter/material.dart';
import '../api.dart';
import '../widgets/bar.dart';
import '../widgets/pick.dart';
import '../widgets/poster.dart';
import '../widgets/states.dart';
import 'calendar.dart';
import 'detail.dart';
import 'library.dart';
import 'shell.dart' show sectionIcon;

void openItem(BuildContext context, Api api, Map<String, dynamic> it) {
  Navigator.push(context, MaterialPageRoute(builder: (_) => DetailScreen(api: api, id: '${it['ratingKey']}')));
}

/// The start screen: the libraries as chips, the rows (continue watching, recently added, ...) and the search.
class HomeScreen extends StatefulWidget {
  final Api api;
  const HomeScreen({super.key, required this.api});
  @override
  State<HomeScreen> createState() => _HomeScreenState();
}

class _HomeScreenState extends State<HomeScreen> {
  late Future<List<dynamic>> data = load();
  final search = TextEditingController();
  List<Map<String, dynamic>>? results;
  bool searching = false;
  bool searchOpen = false; // phones: the search field is shown in the title
  Timer? typing;
  int searchGen = 0; // a slow answer to an older search is dropped

  @override
  void dispose() {
    typing?.cancel();
    search.dispose();
    super.dispose();
  }

  // "Coming soon" from its own address; an older server without it gives an empty row.
  Future<List<dynamic>> load() => Future.wait([
        widget.api.hubs(),
        widget.api.sections(),
        widget.api.upcoming().catchError((_) => <Map<String, dynamic>>[]),
      ]);

  Future<void> reload() async {
    setState(() => data = load());
    await data;
  }

  /// Searches a moment after the typing stops (Enter searches at once).
  void typed(String q) {
    typing?.cancel();
    typing = Timer(const Duration(milliseconds: 300), () => doSearch(q));
  }

  Future<void> doSearch(String q) async {
    typing?.cancel();
    final my = ++searchGen;
    if (q.trim().isEmpty) {
      return setState(() {
        results = null;
        searching = false;
      });
    }
    setState(() => searching = true);
    try {
      final r = await widget.api.search(q.trim());
      if (mounted && my == searchGen) setState(() => results = r);
    } catch (e) {
      if (mounted && my == searchGen) ScaffoldMessenger.of(context).showSnackBar(SnackBar(content: Text(errorText(e))));
    } finally {
      if (mounted && my == searchGen) setState(() => searching = false);
    }
  }

  void closeSearch() => setState(() {
        searchOpen = false;
        searchGen++;
        typing?.cancel();
        search.clear();
        results = null;
        searching = false;
      });

  @override
  Widget build(BuildContext context) {
    final api = widget.api;
    final narrow = isNarrow(context);
    final field = TextField(
      controller: search,
      autofocus: narrow,
      textInputAction: TextInputAction.search,
      onChanged: typed,
      onSubmitted: doSearch,
      decoration: InputDecoration(
        hintText: 'Suchen…',
        prefixIcon: const Icon(Icons.search),
        suffixIcon: searching
            ? const Padding(padding: EdgeInsets.all(14), child: SizedBox(width: 16, height: 16, child: CircularProgressIndicator(strokeWidth: 2)))
            : (!narrow && search.text.isNotEmpty ? IconButton(tooltip: 'Leeren', icon: const Icon(Icons.close), onPressed: closeSearch) : null),
        border: InputBorder.none,
      ),
    );
    return Scaffold(
      appBar: AppBar(
        // On a phone the search takes the title's place.
        title: narrow && searchOpen ? field : const Text('Lichtspiel'),
        actions: [
          if (narrow)
            IconButton(
              tooltip: searchOpen ? 'Suche schließen' : 'Suchen',
              icon: Icon(searchOpen ? Icons.close : Icons.search),
              onPressed: () => searchOpen ? closeSearch() : setState(() => searchOpen = true),
            )
          else
            SizedBox(width: 300, child: field),
          if (!narrow) IconButton(tooltip: 'Aktualisieren', icon: const Icon(Icons.refresh), onPressed: reload),
        ],
      ),
      body: results != null
          ? results!.isEmpty
              ? const ErrorView(error: 'Nichts gefunden.', icon: Icons.search_off)
              : ListView(padding: const EdgeInsets.all(16), children: [
                  Wrap(spacing: 12, runSpacing: 16, children: [for (final i in results!) Poster(api: api, item: i, onTap: () => openItem(context, api, i))]),
                ])
          : FutureBuilder<List<dynamic>>(
              future: data,
              builder: (context, snap) {
                if (snap.hasError) return ErrorView(error: snap.error!, onRetry: reload);
                if (!snap.hasData) return const RowsSkeleton();
                final hubs = snap.data![0] as List<Map<String, dynamic>>;
                final sections = snap.data![1] as List<Map<String, dynamic>>;
                final coming = snap.data![2] as List<Map<String, dynamic>>;
                // After continue watching and the watchlist, like in the web app.
                final comingAt = hubs.indexWhere((h) => !['continueWatching', 'watchlist'].contains(h['hubIdentifier']));
                final before = comingAt < 0 ? hubs : hubs.sublist(0, comingAt), after = comingAt < 0 ? const <Map<String, dynamic>>[] : hubs.sublist(comingAt);
                // "Etwas Kurzes" sits on the first row it picks from (continue watching, else the watchlist).
                final shortOn = hubs.where((h) => ['continueWatching', 'watchlist'].contains(h['hubIdentifier']) && ((h['Metadata'] as List?) ?? const []).isNotEmpty).firstOrNull;
                return RefreshIndicator(
                  onRefresh: reload,
                  child: ListView(padding: const EdgeInsets.symmetric(vertical: 12), children: [
                    Padding(
                      padding: const EdgeInsets.symmetric(horizontal: 16),
                      child: Wrap(spacing: 8, runSpacing: 8, children: [
                        for (final s in sections)
                          ActionChip(
                            avatar: Icon(sectionIcon(s['type'])),
                            label: Text('${s['title']}'),
                            onPressed: () => Navigator.push(context, MaterialPageRoute(builder: (_) => LibraryScreen(api: api, section: s))),
                          ),
                      ]),
                    ),
                    for (final hub in before) _row(context, api, hub, short: hub == shortOn),
                    if (coming.isNotEmpty) ComingRow(api: api, entries: coming),
                    for (final hub in after) _row(context, api, hub, short: hub == shortOn),
                    if (hubs.isEmpty) const Padding(padding: EdgeInsets.all(32), child: Center(child: Text('Noch nichts da. Die Bibliotheken werden evtl. noch gescannt.'))),
                  ]),
                );
              },
            ),
    );
  }

  Widget _row(BuildContext context, Api api, Map<String, dynamic> hub, {bool short = false}) {
    final items = ((hub['Metadata'] as List?) ?? const []).map((e) => Map<String, dynamic>.from(e)).toList();
    if (items.isEmpty) return const SizedBox.shrink();
    final title = Text('${hub['title']}', style: Theme.of(context).textTheme.titleMedium);
    return Column(crossAxisAlignment: CrossAxisAlignment.start, children: [
      short
          ? Padding(
              padding: const EdgeInsets.fromLTRB(16, 12, 8, 0),
              child: Row(children: [
                Expanded(child: title),
                TextButton.icon(onPressed: () => shortDialog(context, api), icon: const Icon(Icons.timer_outlined, size: 18), label: const Text('Etwas Kurzes')),
              ]),
            )
          : Padding(padding: const EdgeInsets.fromLTRB(16, 20, 16, 8), child: title),
      SizedBox(
        height: 280,
        child: ListView.separated(
          scrollDirection: Axis.horizontal,
          padding: const EdgeInsets.symmetric(horizontal: 16),
          itemCount: items.length,
          separatorBuilder: (_, _) => const SizedBox(width: 12),
          itemBuilder: (_, i) => Poster(
            api: api,
            item: items[i],
            onTap: () => openItem(context, api, items[i]),
            onRemoved: hub['hubIdentifier'] == 'continueWatching' ? reload : null,
          ),
        ),
      ),
    ]);
  }
}
