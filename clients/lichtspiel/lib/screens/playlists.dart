import 'dart:math';
import 'package:flutter/material.dart';
import '../api.dart';
import '../widgets/bar.dart';
import '../widgets/poster.dart';
import 'detail.dart';
import 'library.dart';
import 'player.dart';
import 'smart.dart';

String _dur(dynamic ms) {
  final m = ((ms is num ? ms : 0) / 60000).round();
  if (m <= 0) return '';
  return m >= 60 ? '${m ~/ 60} Std. ${m % 60} Min.' : '$m Min.';
}

String plTitle(Map<String, dynamic> p) => '${p['title']}';

/// Picks a playlist (or a new one) and adds the item; used by the detail page.
/// With [item] (a movie or show) the owner can also put it into a collection of its library.
Future<void> addToPlaylistDialog(BuildContext context, Api api, String itemId, {List<String>? ids, Map<String, dynamic>? item}) async {
  final msg = ScaffoldMessenger.of(context);
  final List<Map<String, dynamic>> pls;
  try {
    pls = await api.playlists(kind: 'manual', owner: 'me', sort: 'title');
  } catch (e) {
    msg.showSnackBar(SnackBar(content: Text('$e')));
    return;
  }
  var mine = <String>[];
  var cols = <String>[];
  final collectable = Poster.admin && item != null && ids == null && (item['type'] == 'movie' || item['type'] == 'show') && item['librarySectionID'] != null;
  if (collectable) {
    try {
      final full = await api.item(itemId);
      mine = [for (final c in (full['extra']?['collections'] as List? ?? const [])) '$c'];
      cols = [for (final c in await api.sectionGroups('${item['librarySectionID']}', 'collections')) '${c['title']}'].where((c) => !mine.contains(c)).toList();
    } catch (_) {}
  }
  if (!context.mounted) return;
  final choice = await showDialog<Object>(
    context: context,
    builder: (c) => SimpleDialog(title: const Text('Zur Playlist hinzufügen'), children: [
      SimpleDialogOption(onPressed: () => Navigator.pop(c, 'new'), child: const Row(children: [Icon(Icons.add), SizedBox(width: 8), Text('Neue Playlist…')])),
      for (final p in pls) SimpleDialogOption(onPressed: () => Navigator.pop(c, p), child: Text(plTitle(p))),
      if (collectable) ...[
        const Divider(),
        for (final name in cols) SimpleDialogOption(onPressed: () => Navigator.pop(c, 'coll:$name'), child: Text('Sammlung: $name')),
        SimpleDialogOption(onPressed: () => Navigator.pop(c, 'newcoll'), child: const Row(children: [Icon(Icons.add), SizedBox(width: 8), Text('Neue Sammlung…')])),
      ],
    ]),
  );
  if (choice == null || !context.mounted) return;
  try {
    if (choice == 'newcoll' || (choice is String && choice.startsWith('coll:'))) {
      var name = choice is String && choice.startsWith('coll:') ? choice.substring(5) : '';
      if (choice == 'newcoll') {
        name = (await askText(context, 'Name der neuen Sammlung')) ?? '';
        if (name.isEmpty) return;
      }
      await api.put('/library/metadata/$itemId', {'collections': [...mine, name]});
      msg.showSnackBar(SnackBar(content: Text('Zu „$name“ hinzugefügt')));
    } else if (choice == 'new') {
      final name = await askText(context, 'Name der neuen Playlist');
      if (name == null || name.isEmpty) return;
      await api.createPlaylist(name, ids: ids ?? [itemId]);
      msg.showSnackBar(SnackBar(content: Text('Zu „$name“ hinzugefügt')));
    } else {
      final p = choice as Map<String, dynamic>;
      final added = await api.addToPlaylist('${p['id']}', itemId, ids: ids);
      msg.showSnackBar(SnackBar(content: Text(added ? 'Zu „${plTitle(p)}“ hinzugefügt' : 'Schon in dieser Playlist')));
    }
  } catch (e) {
    msg.showSnackBar(SnackBar(content: Text('$e')));
  }
}

Future<String?> askText(BuildContext context, String title, {String value = ''}) {
  final c = TextEditingController(text: value);
  return showDialog<String>(
    context: context,
    builder: (d) => AlertDialog(
      title: Text(title),
      content: TextField(controller: c, autofocus: true, onSubmitted: (v) => Navigator.pop(d, v.trim())),
      actions: [
        TextButton(onPressed: () => Navigator.pop(d), child: const Text('Abbrechen')),
        FilledButton(onPressed: () => Navigator.pop(d, c.text.trim()), child: const Text('OK')),
      ],
    ),
  );
}

Future<bool> _sure(BuildContext context, String text) async =>
    await showDialog<bool>(
      context: context,
      builder: (d) => AlertDialog(
        content: Text(text),
        actions: [TextButton(onPressed: () => Navigator.pop(d, false), child: const Text('Abbrechen')), FilledButton(onPressed: () => Navigator.pop(d, true), child: const Text('Ja'))],
      ),
    ) ??
    false;

/// Plays a playlist from the start; [shuffle] avoids the same show twice in a row while others are left.
Future<void> playPlaylist(BuildContext context, Api api, String id, {bool shuffle = false}) async {
  List<Map<String, dynamic>> items;
  try {
    items = await api.playlistItems(id);
  } catch (e) {
    if (context.mounted) ScaffoldMessenger.of(context).showSnackBar(SnackBar(content: Text('$e')));
    return;
  }
  if (!context.mounted) return;
  if (items.isEmpty) {
    ScaffoldMessenger.of(context).showSnackBar(const SnackBar(content: Text('Diese Playlist ist leer')));
    return;
  }
  if (shuffle) items = smartShuffle(items);
  Navigator.push(context, MaterialPageRoute(builder: (_) => PlayerScreen(api: api, item: items.first, queue: items.sublist(1))));
}

List<Map<String, dynamic>> smartShuffle(List<Map<String, dynamic>> src) {
  final left = [...src]..shuffle(Random());
  final out = <Map<String, dynamic>>[];
  while (left.isNotEmpty) {
    final last = out.isEmpty ? null : out.last['grandparentRatingKey'];
    var k = last == null ? 0 : left.indexWhere((e) => e['grandparentRatingKey'] != last);
    if (k < 0) k = 0;
    out.add(left.removeAt(k));
  }
  return out;
}

class PlaylistsScreen extends StatefulWidget {
  final Api api;
  final String section; // only playlists with something from this library
  const PlaylistsScreen({super.key, required this.api, this.section = ''});
  @override
  State<PlaylistsScreen> createState() => _PlaylistsScreenState();
}

class _PlaylistsScreenState extends State<PlaylistsScreen> {
  String kind = '', owner = '', sort = 'pinned:desc,title';
  List<Map<String, dynamic>>? list;
  String? error;

  static const kinds = {'': 'Alle Arten', 'default': 'Standard', 'smart': 'Intelligent', 'manual': 'Normal'};
  static const owners = {'': 'Von allen', 'me': 'Meine', 'others': 'Mit mir geteilt', 'server': 'Vom Server'};
  static const sorts = {
    'pinned:desc,title': 'Angeheftete zuerst',
    'title': 'Name A–Z',
    'title:desc': 'Name Z–A',
    'updated_at:desc': 'Zuletzt geändert',
    'created_at:desc': 'Neueste',
    'item_count:desc': 'Meiste Einträge',
    'duration:desc': 'Längste',
  };

  @override
  void initState() {
    super.initState();
    reload();
  }

  Future<void> reload() async {
    try {
      final r = await widget.api.playlists(kind: kind, owner: owner, sort: sort, section: widget.section);
      if (mounted) setState(() => list = r);
    } catch (e) {
      if (mounted) setState(() => error = '$e');
    }
  }

  Widget menu(Map<String, String> opts, String cur, void Function(String) on) => PopupMenuButton<String>(
        onSelected: (v) {
          on(v);
          reload();
        },
        itemBuilder: (_) => [for (final e in opts.entries) CheckedPopupMenuItem(value: e.key, checked: e.key == cur, child: Text(e.value))],
        child: Chip(label: Text(opts[cur] ?? cur)),
      );

  @override
  Widget build(BuildContext context) {
    final l = list;
    return Scaffold(
      appBar: AppBar(title: const Text('Playlists'), actions: barActions(context, labels: true, keep: 2, [
        BarAction(Icons.add, 'Neue Playlist', () async {
          final name = await askText(context, 'Name der neuen Playlist');
          if (name == null || name.isEmpty) return;
          final Map<String, dynamic> p;
          try {
            p = await widget.api.createPlaylist(name);
          } catch (e) {
            if (context.mounted) ScaffoldMessenger.of(context).showSnackBar(SnackBar(content: Text('$e')));
            return;
          }
          if (!mounted) return;
          await Navigator.push(context, MaterialPageRoute(builder: (_) => PlaylistScreen(api: widget.api, id: '${p['id']}')));
          reload();
        }),
        BarAction(Icons.auto_awesome_outlined, 'Neue intelligente Playlist', () async {
          final id = await smartEditor(context, widget.api);
          if (id == null || !mounted) return;
          await Navigator.push(context, MaterialPageRoute(builder: (_) => PlaylistScreen(api: widget.api, id: id)));
          reload();
        }),
      ])),
      body: Column(children: [
        Padding(
          padding: const EdgeInsets.fromLTRB(16, 8, 16, 0),
          child: Wrap(spacing: 8, runSpacing: 8, children: [
            menu(kinds, kind, (v) => kind = v),
            menu(owners, owner, (v) => owner = v),
            menu(sorts, sort, (v) => sort = v),
          ]),
        ),
        Expanded(
          child: error != null
              ? Center(child: Text(error!))
              : l == null
                  ? const Center(child: CircularProgressIndicator())
                  : l.isEmpty
                      ? const Center(child: Text('Keine passenden Playlists.'))
                      : ListView(children: [
                          for (final p in l)
                            ListTile(
                              leading: p['thumbs'] is List && (p['thumbs'] as List).isNotEmpty
                                  ? ClipRRect(borderRadius: BorderRadius.circular(4), child: Image.network(widget.api.img('${(p['thumbs'] as List).first}'), width: 40, height: 60, fit: BoxFit.cover, errorBuilder: (_, __, ___) => const Icon(Icons.playlist_play)))
                                  : const Icon(Icons.playlist_play, size: 36),
                              title: Text(plTitle(p)),
                              subtitle: Text([
                                {'manual': 'normal', 'smart': 'intelligent', 'default': 'Standard'}[p['kind']] ?? '${p['kind']}',
                                '${p['leafCount'] ?? 0} Einträge',
                                _dur(p['duration']),
                                if (p['shared'] == true && p['kind'] != 'default') 'geteilt',
                                if (p['pinned'] == true) 'auf der Startseite',
                              ].where((e) => e.isNotEmpty).join(' · ')),
                              trailing: (p['leafCount'] ?? 0) > 0 ? IconButton(tooltip: 'Abspielen', icon: const Icon(Icons.play_arrow), onPressed: () => playPlaylist(context, widget.api, '${p['id']}')) : null,
                              onTap: () async {
                                await Navigator.push(context, MaterialPageRoute(builder: (_) => PlaylistScreen(api: widget.api, id: '${p['id']}')));
                                reload();
                              },
                            ),
                        ]),
        ),
      ]),
    );
  }
}

class PlaylistScreen extends StatefulWidget {
  final Api api;
  final String id;
  const PlaylistScreen({super.key, required this.api, required this.id});
  @override
  State<PlaylistScreen> createState() => _PlaylistScreenState();
}

class _PlaylistScreenState extends State<PlaylistScreen> {
  Map<String, dynamic>? pl;
  List<Map<String, dynamic>> items = [];
  String? error;
  String watched = '', q = '';

  Api get api => widget.api;

  @override
  void initState() {
    super.initState();
    reload();
  }

  Future<void> reload() async {
    try {
      final p = await api.playlist(widget.id);
      final r = await api.playlistItems(widget.id);
      if (mounted) {
        setState(() {
          pl = p;
          items = r;
        });
      }
    } catch (e) {
      if (mounted) setState(() => error = '$e');
    }
  }

  dynamic entryOf(Map<String, dynamic> it) => it['id'] ?? it['ratingKey'];

  Future<void> move(int i, int dir) async {
    final t = i + dir;
    if (t < 0 || t >= items.length) return;
    final after = dir < 0 ? (t == 0 ? 0 : entryOf(items[t - 1])) : entryOf(items[t]);
    try {
      await api.moveInPlaylist(widget.id, '${entryOf(items[i])}', after);
    } catch (e) {
      if (mounted) ScaffoldMessenger.of(context).showSnackBar(SnackBar(content: Text('$e')));
    }
    reload();
  }

  String label(Map<String, dynamic> it) => it['type'] == 'episode'
      ? '${it['grandparentTitle']} · S${it['parentIndex']}E${it['index']} ${it['title']}'
      : it['type'] == 'track'
          ? '${it['grandparentTitle'] ?? ''} · ${it['title']}'
          : '${it['title']}';

  @override
  Widget build(BuildContext context) {
    final p = pl;
    if (error != null) return Scaffold(appBar: AppBar(), body: Center(child: Text(error!)));
    if (p == null) return Scaffold(appBar: AppBar(), body: const Center(child: CircularProgressIndicator()));
    final editable = p['editable'] == true, manual = p['kind'] == 'manual', isDefault = p['default'] == true;
    final shown = items.where((it) {
      if (q.isNotEmpty && !'${it['title']}'.toLowerCase().contains(q.toLowerCase())) return false;
      final w = ((it['viewCount'] ?? 0) as num) > 0;
      if (watched == 'watched' && !w) return false;
      if (watched == 'unwatched' && w) return false;
      return true;
    }).toList();
    final reorder = manual && editable && q.isEmpty && watched.isEmpty;
    return Scaffold(
      appBar: AppBar(title: Text(plTitle(p)), actions: [
        PopupMenuButton<String>(
          onSelected: (v) async {
            try {
              switch (v) {
                case 'rename':
                  final n = await askText(context, 'Neuer Name', value: plTitle(p));
                  if (n != null && n.isNotEmpty) await api.updatePlaylist(widget.id, {'title': n});
                case 'pin':
                  await api.updatePlaylist(widget.id, {'pinned': p['pinned'] != true});
                case 'share':
                  await api.updatePlaylist(widget.id, {'shared': p['shared'] != true});
                case 'rules':
                  await smartEditor(context, api, playlist: p);
                case 'copy':
                  final r = await api.post('/playlists/${widget.id}/copy', {});
                  if (mounted && r is Map) {
                    await Navigator.pushReplacement(context, MaterialPageRoute(builder: (_) => PlaylistScreen(api: api, id: '${r['id']}')));
                    return;
                  }
                case 'delete':
                  if (await _sure(context, 'Die Playlist „${plTitle(p)}“ löschen?')) {
                    await api.deletePlaylist(widget.id);
                    if (mounted) Navigator.pop(context);
                    return;
                  }
              }
            } catch (e) {
              if (mounted) ScaffoldMessenger.of(context).showSnackBar(SnackBar(content: Text('$e')));
            }
            reload();
          },
          itemBuilder: (_) => [
            if (editable) const PopupMenuItem(value: 'rename', child: Text('Umbenennen')),
            if (editable) PopupMenuItem(value: 'pin', child: Text(p['pinned'] == true ? 'Von der Startseite entfernen' : 'Auf der Startseite zeigen')),
            if (editable && !isDefault) PopupMenuItem(value: 'share', child: Text(p['shared'] == true ? 'Nicht mehr teilen' : 'Mit allen teilen')),
            if (p['smart'] == true && editable) const PopupMenuItem(value: 'rules', child: Text('Regeln bearbeiten')),
            if (p['smart'] == true) const PopupMenuItem(value: 'copy', child: Text('Als normale Playlist speichern')),
            if (editable && !isDefault) const PopupMenuItem(value: 'delete', child: Text('Löschen')),
          ],
        ),
      ]),
      body: Column(crossAxisAlignment: CrossAxisAlignment.start, children: [
        Padding(
          padding: const EdgeInsets.fromLTRB(16, 8, 16, 0),
          child: Wrap(spacing: 8, runSpacing: 8, crossAxisAlignment: WrapCrossAlignment.center, children: [
            FilledButton.icon(onPressed: () => playPlaylist(context, api, widget.id), icon: const Icon(Icons.play_arrow), label: const Text('Alle abspielen')),
            OutlinedButton.icon(onPressed: () => playPlaylist(context, api, widget.id, shuffle: true), icon: const Icon(Icons.shuffle), label: const Text('Zufällig')),
            SegmentedButton<String>(
              showSelectedIcon: false,
              segments: const [ButtonSegment(value: '', label: Text('Alle')), ButtonSegment(value: 'unwatched', label: Text('Ungesehen')), ButtonSegment(value: 'watched', label: Text('Gesehen'))],
              selected: {watched},
              onSelectionChanged: (s) => setState(() => watched = s.first),
            ),
            SizedBox(width: 170, child: TextField(decoration: const InputDecoration(isDense: true, hintText: 'Nach Titel filtern', border: OutlineInputBorder()), onChanged: (v) => setState(() => q = v.trim()))),
            Text('${shown.length} von ${items.length} Einträgen · ${_dur(p['duration'])}', style: const TextStyle(color: Colors.white60)),
          ]),
        ),
        if (p['summary'] != null && '${p['summary']}'.isNotEmpty) Padding(padding: const EdgeInsets.fromLTRB(16, 8, 16, 0), child: Text('${p['summary']}', style: const TextStyle(color: Colors.white60))),
        Expanded(
          child: shown.isEmpty
              ? Center(child: Text(manual ? 'Leer. Öffne einen Film oder eine Folge und wähle „Zur Playlist“.' : 'Aktuell passt nichts zu diesen Regeln.'))
              : ListView.builder(
                  itemCount: shown.length,
                  itemBuilder: (_, n) {
                    final it = shown[n];
                    final i = items.indexOf(it);
                    final thumb = '${it['squareThumb'] ?? (it['type'] == 'episode' && it['grandparentThumb'] != null ? it['grandparentThumb'] : it['thumb']) ?? ''}';
                    return ListTile(
                      leading: thumb.isEmpty ? const Icon(Icons.movie_outlined) : ClipRRect(borderRadius: BorderRadius.circular(4), child: Image.network(api.img(thumb), width: 40, height: 60, fit: BoxFit.cover, errorBuilder: (_, __, ___) => const Icon(Icons.movie_outlined))),
                      title: Text(label(it), maxLines: 1, overflow: TextOverflow.ellipsis),
                      subtitle: Text([if (it['year'] != null) '${it['year']}', _dur(it['duration']), if (((it['viewCount'] ?? 0) as num) > 0) '✓ gesehen'].where((e) => e.isNotEmpty).join(' · ')),
                      onTap: () => Navigator.push(context, MaterialPageRoute(builder: (_) => DetailScreen(api: api, id: '${it['ratingKey']}'))),
                      trailing: Row(mainAxisSize: MainAxisSize.min, children: [
                        if (reorder) ...[
                          IconButton(tooltip: 'Nach oben', icon: const Icon(Icons.arrow_upward, size: 18), onPressed: () => move(i, -1)),
                          IconButton(tooltip: 'Nach unten', icon: const Icon(Icons.arrow_downward, size: 18), onPressed: () => move(i, 1)),
                        ],
                        if (manual && editable)
                          IconButton(
                            tooltip: 'Entfernen',
                            icon: const Icon(Icons.close, size: 18),
                            onPressed: () async {
                              if (!await _sure(context, '„${it['title']}“ aus dieser Playlist entfernen?')) return;
                              try {
                                await api.removeFromPlaylist(widget.id, '${entryOf(it)}');
                              } catch (e) {
                                if (context.mounted) ScaffoldMessenger.of(context).showSnackBar(SnackBar(content: Text('$e')));
                              }
                              reload();
                            },
                          ),
                        IconButton(
                          tooltip: 'Abspielen',
                          icon: const Icon(Icons.play_arrow),
                          onPressed: () => Navigator.push(context, MaterialPageRoute(builder: (_) => PlayerScreen(api: api, item: it, queue: items.sublist(i + 1)))),
                        ),
                      ]),
                    );
                  },
                ),
        ),
      ]),
    );
  }
}

/// Collections or categories of a library as tiles; a tap opens the library filtered by it.
class GroupsScreen extends StatefulWidget {
  final Api api;
  final Map<String, dynamic> section;
  final String kind; // collections | categories
  const GroupsScreen({super.key, required this.api, required this.section, required this.kind});
  @override
  State<GroupsScreen> createState() => _GroupsScreenState();
}

class _GroupsScreenState extends State<GroupsScreen> {
  late final Future<List<Map<String, dynamic>>> data = widget.api.sectionGroups('${widget.section['key']}', widget.kind);

  @override
  Widget build(BuildContext context) {
    final coll = widget.kind == 'collections';
    return Scaffold(
      appBar: AppBar(title: Text(coll ? 'Sammlungen' : 'Kategorien')),
      body: FutureBuilder<List<Map<String, dynamic>>>(
        future: data,
        builder: (context, snap) {
          if (snap.hasError) return Center(child: Text('${snap.error}'));
          if (!snap.hasData) return const Center(child: CircularProgressIndicator());
          final l = snap.data!;
          if (l.isEmpty) return Center(child: Text(coll ? 'Noch keine Sammlungen. Filmreihen kommen beim Herunterladen der Details dazu.' : 'Noch keine Kategorien. Sie stammen aus den Genres.'));
          return GridView.builder(
            padding: const EdgeInsets.all(16),
            gridDelegate: SliverGridDelegateWithMaxCrossAxisExtent(maxCrossAxisExtent: coll ? 170 : 260, childAspectRatio: coll ? 0.55 : 1.8, crossAxisSpacing: 12, mainAxisSpacing: 12),
            itemCount: l.length,
            itemBuilder: (_, i) {
              final d = l[i];
              final pic = '${(coll ? d['thumb'] : d['art'] ?? d['thumb']) ?? ''}';
              final n = d['childCount'] ?? 0;
              return InkWell(
                onTap: () => Navigator.push(context, MaterialPageRoute(builder: (_) => LibraryScreen(api: widget.api, section: widget.section, initialFilters: {coll ? 'collection' : 'genre': '${d['title']}'}))),
                child: Column(crossAxisAlignment: CrossAxisAlignment.start, children: [
                  Expanded(
                    child: ClipRRect(
                      borderRadius: BorderRadius.circular(6),
                      child: SizedBox.expand(
                        child: pic.isEmpty ? const ColoredBox(color: Colors.white10, child: Icon(Icons.collections_bookmark_outlined)) : Image.network(widget.api.img(pic), fit: BoxFit.cover, errorBuilder: (_, __, ___) => const ColoredBox(color: Colors.white10)),
                      ),
                    ),
                  ),
                  const SizedBox(height: 4),
                  Text('${d['title']}', maxLines: 1, overflow: TextOverflow.ellipsis),
                  Text(n == 1 ? '1 Eintrag' : '$n Einträge', style: const TextStyle(fontSize: 12, color: Colors.white60)),
                ]),
              );
            },
          );
        },
      ),
    );
  }
}
