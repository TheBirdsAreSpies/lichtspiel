import 'package:flutter/material.dart';
import '../api.dart';
import '../widgets/activity.dart';
import '../widgets/poster.dart';
import 'dashboard.dart';
import 'detail.dart';
import 'library.dart';
import 'playlists.dart';
import 'settings.dart';
import 'watchlist.dart';

void openItem(BuildContext context, Api api, Map<String, dynamic> it) {
  Navigator.push(context, MaterialPageRoute(builder: (_) => DetailScreen(api: api, id: '${it['ratingKey']}')));
}

class HomeScreen extends StatefulWidget {
  final Api api;
  final VoidCallback onSignOut;
  const HomeScreen({super.key, required this.api, required this.onSignOut});
  @override
  State<HomeScreen> createState() => _HomeScreenState();
}

class _HomeScreenState extends State<HomeScreen> {
  late Future<List<dynamic>> data = Future.wait([widget.api.hubs(), widget.api.sections()]);
  final search = TextEditingController();
  List<Map<String, dynamic>>? results;
  bool admin = false;

  @override
  void initState() {
    super.initState();
    widget.api.get('/api/me').then((m) {
      if (mounted && m is Map) setState(() => Poster.admin = admin = m['admin'] == true);
      if (mounted && m is Map && m['admin'] == true) Activity.start(widget.api);
    }).catchError((_) {});
  }

  @override
  void dispose() {
    Activity.stop();
    super.dispose();
  }

  Future<void> reload() async {
    setState(() => data = Future.wait([widget.api.hubs(), widget.api.sections()]));
    await data;
  }

  Future<void> doSearch(String q) async {
    if (q.trim().isEmpty) return setState(() => results = null);
    final r = await widget.api.search(q.trim());
    if (mounted) setState(() => results = r);
  }

  @override
  Widget build(BuildContext context) {
    final api = widget.api;
    return Scaffold(
      appBar: AppBar(
        title: const Text('Lichtspiel'),
        actions: [
          SizedBox(
            width: 260,
            child: TextField(
              controller: search,
              onSubmitted: doSearch,
              decoration: const InputDecoration(hintText: 'Suchen…', prefixIcon: Icon(Icons.search), border: InputBorder.none),
            ),
          ),
          IconButton(tooltip: 'Merkliste', icon: const Icon(Icons.bookmark_border), onPressed: () => Navigator.push(context, MaterialPageRoute(builder: (_) => WatchlistScreen(api: api)))),
          IconButton(tooltip: 'Playlists', icon: const Icon(Icons.playlist_play), onPressed: () => Navigator.push(context, MaterialPageRoute(builder: (_) => PlaylistsScreen(api: api)))),
          if (admin) IconButton(tooltip: 'Dashboard', icon: const Icon(Icons.monitor_heart_outlined), onPressed: () => Navigator.push(context, MaterialPageRoute(builder: (_) => DashboardScreen(api: api)))),
          IconButton(tooltip: 'Einstellungen', icon: const Icon(Icons.settings_outlined), onPressed: () => Navigator.push(context, MaterialPageRoute(builder: (_) => SettingsScreen(api: api, admin: admin)))),
          IconButton(tooltip: 'Aktualisieren', icon: const Icon(Icons.refresh), onPressed: reload),
          IconButton(tooltip: 'Abmelden', icon: const Icon(Icons.logout), onPressed: widget.onSignOut),
        ],
      ),
      body: results != null
          ? ListView(padding: const EdgeInsets.all(16), children: [
              Wrap(spacing: 12, runSpacing: 16, children: [for (final i in results!) Poster(api: api, item: i, onTap: () => openItem(context, api, i))]),
              if (results!.isEmpty) const Center(child: Text('Nichts gefunden.')),
            ])
          : FutureBuilder<List<dynamic>>(
              future: data,
              builder: (context, snap) {
                if (snap.hasError) return Center(child: Text('${snap.error}'));
                if (!snap.hasData) return const Center(child: CircularProgressIndicator());
                final hubs = snap.data![0] as List<Map<String, dynamic>>;
                final sections = snap.data![1] as List<Map<String, dynamic>>;
                return RefreshIndicator(
                  onRefresh: reload,
                  child: ListView(padding: const EdgeInsets.symmetric(vertical: 12), children: [
                    Padding(
                      padding: const EdgeInsets.symmetric(horizontal: 16),
                      child: Wrap(spacing: 8, children: [
                        for (final s in sections)
                          ActionChip(
                            avatar: Icon(s['type'] == 'show' ? Icons.tv : Icons.movie),
                            label: Text('${s['title']}'),
                            onPressed: () => Navigator.push(context, MaterialPageRoute(builder: (_) => LibraryScreen(api: api, section: s))),
                          ),
                      ]),
                    ),
                    for (final hub in hubs) _row(context, api, hub),
                    if (hubs.isEmpty) const Padding(padding: EdgeInsets.all(32), child: Center(child: Text('Noch nichts da. Die Bibliotheken werden evtl. noch gescannt.'))),
                  ]),
                );
              },
            ),
    );
  }

  Widget _row(BuildContext context, Api api, Map<String, dynamic> hub) {
    final items = ((hub['Metadata'] as List?) ?? const []).map((e) => Map<String, dynamic>.from(e)).toList();
    if (items.isEmpty) return const SizedBox.shrink();
    return Column(crossAxisAlignment: CrossAxisAlignment.start, children: [
      Padding(padding: const EdgeInsets.fromLTRB(16, 20, 16, 8), child: Text('${hub['title']}', style: Theme.of(context).textTheme.titleMedium)),
      SizedBox(
        height: 280,
        child: ListView.separated(
          scrollDirection: Axis.horizontal,
          padding: const EdgeInsets.symmetric(horizontal: 16),
          itemCount: items.length,
          separatorBuilder: (_, __) => const SizedBox(width: 12),
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
