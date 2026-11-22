import 'package:flutter/material.dart';
import '../api.dart';
import '../widgets/activity.dart';
import '../widgets/bar.dart';
import '../widgets/poster.dart';
import '../widgets/queue.dart';
import '../widgets/states.dart';
import '../widgets/together.dart';
import '../theme.dart';
import 'dashboard.dart';
import 'downloads.dart';
import 'home.dart';
import 'library.dart';
import 'playlists.dart';
import 'settings.dart';
import 'stats.dart';
import 'watchlist.dart';

/// The icon of a library by its kind.
IconData sectionIcon(Object? type) => switch ('$type') {
      'show' => Icons.tv,
      'movie' => Icons.movie_outlined,
      'music' || 'artist' => Icons.library_music_outlined,
      'photo' => Icons.photo_library_outlined,
      _ => Icons.video_library_outlined,
    };

/// The frame around the main screens: Start, Bibliotheken, Merkliste, Playlists and Mehr, as a bar at the
/// bottom on a phone and a rail at the side on wider screens.
class AppShell extends StatefulWidget {
  final Api api;
  final VoidCallback onSignOut;
  const AppShell({super.key, required this.api, required this.onSignOut});
  @override
  State<AppShell> createState() => _AppShellState();
}

class _AppShellState extends State<AppShell> {
  int tab = 0;
  bool admin = false;

  /// Bumped when a tab is chosen again, so the watchlist and the playlists show what changed meanwhile.
  final version = [0, 0, 0, 0, 0];
  final built = <int>{0}; // tabs are built the first time they are chosen

  @override
  void initState() {
    super.initState();
    widget.api.get('/api/me').then((m) {
      if (mounted && m is Map) {
        DownloadRights.allowed = m['can_download'] == true;
        DownloadRights.convert = m['admin'] == true || m['allow_transcode'] != false;
        setState(() => Poster.admin = admin = m['admin'] == true);
      }
      if (mounted && m is Map && m['admin'] == true) Activity.start(widget.api);
    }).catchError((_) {});
    TogetherInvites.start(context, widget.api);
  }

  @override
  void dispose() {
    Activity.stop();
    TogetherInvites.stop();
    super.dispose();
  }

  void go(int i) => setState(() {
        if (i != tab && (i == 2 || i == 3)) version[i]++;
        tab = i;
        built.add(i);
      });

  static const _dests = [
    (Icons.home_outlined, Icons.home, 'Start'),
    (Icons.video_library_outlined, Icons.video_library, 'Bibliotheken'),
    (Icons.bookmark_border, Icons.bookmark, 'Merkliste'),
    (Icons.playlist_play_outlined, Icons.playlist_play, 'Playlists'),
    (Icons.more_horiz, Icons.more_horiz, 'Mehr'),
  ];

  Widget page(int i) {
    final api = widget.api;
    return switch (i) {
      0 => HomeScreen(api: api),
      1 => SectionsScreen(api: api),
      2 => WatchlistScreen(key: ValueKey('w${version[2]}'), api: api),
      3 => PlaylistsScreen(key: ValueKey('p${version[3]}'), api: api),
      _ => MoreScreen(api: api, admin: admin, onSignOut: widget.onSignOut),
    };
  }

  @override
  Widget build(BuildContext context) {
    final narrow = isNarrow(context);
    final body = IndexedStack(index: tab, children: [for (var i = 0; i < _dests.length; i++) built.contains(i) ? page(i) : const SizedBox.shrink()]);
    return PopScope(
      // Back on another tab goes to Start first, then leaves the app.
      canPop: tab == 0,
      onPopInvokedWithResult: (didPop, _) {
        if (!didPop) go(0);
      },
      child: narrow
          ? Scaffold(
              body: body,
              bottomNavigationBar: NavigationBar(
                selectedIndex: tab,
                onDestinationSelected: go,
                labelBehavior: NavigationDestinationLabelBehavior.alwaysShow,
                destinations: [for (final d in _dests) NavigationDestination(icon: Icon(d.$1), selectedIcon: Icon(d.$2), label: d.$3)],
              ),
            )
          : Scaffold(
              body: Row(children: [
                NavigationRail(
                  selectedIndex: tab,
                  onDestinationSelected: go,
                  labelType: NavigationRailLabelType.all,
                  destinations: [for (final d in _dests) NavigationRailDestination(icon: Icon(d.$1), selectedIcon: Icon(d.$2), label: Text(d.$3))],
                ),
                VerticalDivider(width: 1, color: context.hair),
                Expanded(child: body),
              ]),
            ),
    );
  }
}

/// All libraries the signed-in person may see.
class SectionsScreen extends StatefulWidget {
  final Api api;
  const SectionsScreen({super.key, required this.api});
  @override
  State<SectionsScreen> createState() => _SectionsScreenState();
}

class _SectionsScreenState extends State<SectionsScreen> {
  late Future<List<Map<String, dynamic>>> data = widget.api.sections();

  Future<void> reload() async {
    setState(() => data = widget.api.sections());
    await data;
  }

  String count(Map<String, dynamic> s) {
    final n = s['count'] ?? s['videoCount'];
    if (n == null) return '';
    return switch ('${s['type']}') {
      'show' => '$n Serien',
      'movie' => '$n Filme',
      'music' || 'artist' => '$n Künstler',
      _ => '$n Einträge',
    };
  }

  @override
  Widget build(BuildContext context) {
    return Scaffold(
      appBar: AppBar(title: const Text('Bibliotheken')),
      body: FutureBuilder<List<Map<String, dynamic>>>(
        future: data,
        builder: (context, snap) {
          if (snap.hasError) return ErrorView(error: snap.error!, onRetry: reload);
          if (!snap.hasData) return const Center(child: CircularProgressIndicator());
          final secs = snap.data!;
          if (secs.isEmpty) return const ErrorView(error: 'Noch keine Bibliothek, die du sehen darfst.');
          return RefreshIndicator(
            onRefresh: reload,
            child: ListView(padding: const EdgeInsets.all(12), children: [
              for (final s in secs)
                Card(
                  child: ListTile(
                    leading: CircleAvatar(backgroundColor: context.hair, child: Icon(sectionIcon(s['type']), color: context.accent)),
                    title: Text('${s['title']}', style: const TextStyle(fontWeight: FontWeight.w600)),
                    subtitle: count(s).isEmpty ? null : Text(count(s)),
                    trailing: const Icon(Icons.chevron_right),
                    onTap: () => Navigator.push(context, MaterialPageRoute(builder: (_) => LibraryScreen(api: widget.api, section: s))),
                  ),
                ),
            ]),
          );
        },
      ),
    );
  }
}

/// Everything that is not a main screen: queue, watching together, downloads, dashboard, settings, signing out.
class MoreScreen extends StatelessWidget {
  final Api api;
  final bool admin;
  final VoidCallback onSignOut;
  const MoreScreen({super.key, required this.api, required this.admin, required this.onSignOut});

  Future<void> signOut(BuildContext context) async {
    final ok = await showDialog<bool>(
      context: context,
      builder: (d) => AlertDialog(
        title: const Text('Abmelden?'),
        content: Text('Du meldest dich auf diesem Gerät von ${Uri.tryParse(api.base)?.host ?? api.base} ab.'),
        actions: [
          TextButton(onPressed: () => Navigator.pop(d, false), child: const Text('Abbrechen')),
          FilledButton(onPressed: () => Navigator.pop(d, true), child: const Text('Abmelden')),
        ],
      ),
    );
    if (ok == true) onSignOut();
  }

  @override
  Widget build(BuildContext context) {
    void push(Widget w) => Navigator.push(context, MaterialPageRoute(builder: (_) => w));
    return Scaffold(
      appBar: AppBar(title: const Text('Mehr')),
      body: ListView(children: [
        ListTile(leading: const Icon(Icons.queue_music), title: const Text('Warteschlange'), onTap: () => showQueueDialog(context, api)),
        ListTile(leading: const Icon(Icons.groups_outlined), title: const Text('Zusammen schauen'), subtitle: const Text('Einem Raum mit Code beitreten'), onTap: () => joinTogetherDialog(context, api)),
        if (DownloadRights.allowed) ListTile(leading: const Icon(Icons.download_outlined), title: const Text('Downloads'), onTap: () => push(DownloadsScreen(api: api))),
        ListTile(leading: const Icon(Icons.insights_outlined), title: const Text('Statistik'), subtitle: const Text('Was du wann und wie viel schaust'), onTap: () => push(StatsScreen(api: api, admin: admin))),
        if (admin) ListTile(leading: const Icon(Icons.monitor_heart_outlined), title: const Text('Dashboard'), onTap: () => push(DashboardScreen(api: api))),
        ListTile(leading: const Icon(Icons.settings_outlined), title: const Text('Einstellungen'), onTap: () => push(SettingsScreen(api: api, admin: admin))),
        const Divider(),
        ListTile(
          leading: Icon(Icons.logout, color: context.bad),
          title: Text('Abmelden', style: TextStyle(color: context.bad)),
          subtitle: Text(api.base, style: TextStyle(color: context.faint)),
          onTap: () => signOut(context),
        ),
      ]),
    );
  }
}
