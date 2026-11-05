import 'package:flutter/material.dart';
import '../api.dart';
import '../widgets/poster.dart';
import 'edit.dart';
import 'player.dart';
import 'home.dart';
import 'playlists.dart';

const _sortFields = {
  'title': 'Titel',
  'year': 'Jahr',
  'release_date': 'Erscheinungsdatum',
  'critic_rating': 'Kritikerwertung',
  'audience_rating': 'Publikumswertung',
  'user_rating': 'Deine Bewertung',
  'content_rating': 'Altersfreigabe',
  'duration': 'Länge',
  'progress': 'Fortschritt',
  'view_count': 'Wiedergaben',
  'added_at': 'Hinzugefügt',
  'last_viewed_at': 'Zuletzt gesehen',
  'resolution': 'Auflösung',
  'bitrate': 'Bitrate',
  'random': 'Zufällig',
};
const _sortAsc = {'title', 'show', 'content_rating', 'random'};

const _filterNames = {
  'genre': 'Genre',
  'year': 'Jahr',
  'decade': 'Jahrzehnt',
  'content_rating': 'Altersfreigabe',
  'collection': 'Sammlung',
  'director': 'Regie',
  'actor': 'Schauspieler',
  'writer': 'Drehbuch',
  'producer': 'Produktion',
  'country': 'Land',
  'studio': 'Studio',
  'network': 'Sender',
  'video_resolution': 'Auflösung',
  'hdr': 'HDR',
  'dovi': 'Dolby Vision',
  'hdr10plus': 'HDR10+',
  'atmos': 'Dolby Atmos',
  'unwatched': 'Ungesehen',
  'unwatched_leaves': 'Ungesehen',
  'in_progress': 'Angefangen',
  'unmatched': 'Nicht zugeordnet',
  'video_codec': 'Video-Codec',
  'audio_codec': 'Audio-Codec',
  'subtitle_codec': 'Untertitelformat',
  'audio_layout': 'Tonkanäle',
  'audio_language': 'Tonsprache',
  'subtitle_language': 'Untertitelsprache',
  'edition': 'Edition',
  'label': 'Label',
  'folder': 'Ordner',
  'duplicate': 'Duplikate',
};
const _groupState = ['unwatched', 'unwatched_leaves', 'in_progress', 'unmatched', 'duplicate'];
const _groupMain = ['genre', 'year', 'decade', 'content_rating', 'collection', 'director', 'actor', 'writer', 'producer', 'country', 'network', 'studio'];
const _resNames = {'4k': '4K', '1080': '1080p', '720': '720p', '480': '480p', 'sd': 'SD'};
const _containers = ['mkv', 'mp4', 'avi', 'mov', 'ts', 'm2ts', 'wmv', 'webm', 'mpg'];

String _filterName(String f) => _filterNames[f] ?? f;
String _valueName(String f, String v) => f == 'video_resolution' ? (_resNames[v] ?? v) : f == 'decade' ? '${v}er' : v;

class LibraryScreen extends StatefulWidget {
  final Api api;
  final Map<String, dynamic> section;
  final Map<String, String> initialFilters;
  const LibraryScreen({super.key, required this.api, required this.section, this.initialFilters = const {}});
  @override
  State<LibraryScreen> createState() => _LibraryScreenState();
}

class _LibraryScreenState extends State<LibraryScreen> {
  static const page = 60;
  final items = <Map<String, dynamic>>[];
  final picked = <String>{}; // ids chosen with the circle on a poster (multiple selection)
  final scroll = ScrollController();
  final search = TextEditingController();
  final from = TextEditingController(), to = TextEditingController();

  // The state, like the web app's: type, sort field and direction, filters by name, year range, title text, file type.
  String type = '';
  String sortField = 'title';
  bool desc = false;
  final filters = <String, String>{};
  String container = '';

  int total = 0;
  int fetched = 0; // how many the server has given us (items can be fewer: a title that moved between two pages is shown once)
  int gen = 0; // bumped on every change so a slow answer to an old question is dropped
  bool loading = false;
  String? error;
  List<({String key, int offset})> letters = [];
  double gridWidth = 0;
  bool jumping = false;
  List<Map<String, dynamic>>? available; // the kinds of filter and their values, loaded when first opened

  String get sectionId => '${widget.section['key']}';
  String get sectionType => '${widget.section['type']}';
  bool get done => fetched >= total && total > 0 || (total == 0 && !loading && error == null && gen > 0 && items.isEmpty);

  @override
  void initState() {
    super.initState();
    filters.addAll(widget.initialFilters);
    scroll.addListener(() {
      if (scroll.hasClients && scroll.position.pixels > scroll.position.maxScrollExtent - 600) loadMore();
    });
    reload();
  }

  String _id(Map<String, dynamic> i) => '${i['ratingKey'] ?? i['id']}';
  List<Map<String, dynamic>> get chosen => [for (final i in items) if (picked.contains(_id(i))) i];

  void say(String m) {
    if (mounted) ScaffoldMessenger.of(context).showSnackBar(SnackBar(content: Text(m)));
  }

  void toggle(Map<String, dynamic> i) => setState(() {
        final id = _id(i);
        if (!picked.remove(id)) picked.add(id);
      });

  /// Everything playable in the chosen titles (the episodes of a show, ...), in library order.
  Future<List<Map<String, dynamic>>> leaves() async {
    const playable = {'movie', 'episode', 'track', 'clip'};
    final out = <Map<String, dynamic>>[];
    for (final i in chosen) {
      if (playable.contains(i['type'])) {
        out.add(i);
      } else {
        out.addAll(await widget.api.allLeaves(_id(i)));
      }
    }
    return out;
  }

  Future<void> playChosen({bool shuffle = false}) async {
    try {
      var list = await leaves();
      if (!mounted) return;
      if (list.isEmpty) return say('Nichts zum Abspielen');
      if (shuffle) list = smartShuffle(list);
      await Navigator.push(context, MaterialPageRoute(builder: (_) => PlayerScreen(api: widget.api, item: list.first, queue: list.sublist(1))));
    } catch (e) {
      say('$e');
    }
  }

  /// Runs [f] for the chosen ones, then tells what happened; [keep] leaves the selection in place.
  Future<void> doChosen(Future<void> Function(List<String> ids) f, String done, {bool keep = false}) async {
    final ids = picked.toList();
    try {
      await f(ids);
      say(done);
      for (final id in ids) {
        final at = items.indexWhere((i) => _id(i) == id);
        if (at >= 0) items[at].addAll(await widget.api.item(id));
      }
      if (!keep) picked.clear();
      if (mounted) setState(() {});
    } catch (e) {
      say('$e');
    }
  }

  Widget? selectionBar() {
    if (picked.isEmpty) return null;
    final admin = Poster.admin;
    final one = picked.length == 1;
    return BottomAppBar(
      height: 64,
      child: SingleChildScrollView(
        scrollDirection: Axis.horizontal,
        child: Row(children: [
          Text(one ? '1 ausgewählt' : '${picked.length} ausgewählt', style: const TextStyle(fontWeight: FontWeight.w600)),
          const SizedBox(width: 8),
          TextButton(onPressed: () => setState(picked.clear), child: const Text('Auswahl aufheben')),
          FilledButton.icon(onPressed: playChosen, icon: const Icon(Icons.play_arrow), label: const Text('Abspielen')),
          const SizedBox(width: 8),
          TextButton(onPressed: () => doChosen((ids) => widget.api.setWatchlistAll(ids, true), 'Auf die Merkliste gesetzt'), child: const Text('+ Merkliste')),
          TextButton(onPressed: () => doChosen((ids) => widget.api.setWatchedAll(ids, true), 'Als gesehen markiert'), child: const Text('Gesehen')),
          TextButton(onPressed: () => doChosen((ids) => widget.api.setWatchedAll(ids, false), 'Als ungesehen markiert'), child: const Text('Ungesehen')),
          TextButton(onPressed: () => addToPlaylistDialog(context, widget.api, picked.first, ids: picked.toList()), child: const Text('Zur Playlist')),
          if (admin && one) TextButton(onPressed: () async { await editDetailsDialog(context, widget.api, await widget.api.item(picked.first)); await doChosen((_) async {}, 'Gespeichert', keep: true); }, child: const Text('Bearbeiten')),
          PopupMenuButton<String>(
            tooltip: 'Mehr',
            onSelected: (v) {
              switch (v) {
                case 'shuffle':
                  playChosen(shuffle: true);
                case 'refresh':
                  doChosen((ids) async { for (final id in ids) { await widget.api.post('/library/metadata/$id/refresh'); } }, 'Details werden geladen', keep: true);
                case 'analyze':
                  doChosen((ids) async { for (final id in ids) { await widget.api.post('/library/metadata/$id/analyze'); } }, 'Wird analysiert', keep: true);
                case 'all':
                  setState(() => picked.addAll(items.map(_id)));
              }
            },
            itemBuilder: (_) => [
              const PopupMenuItem(value: 'shuffle', child: Text('Zufallswiedergabe')),
              if (admin) const PopupMenuItem(value: 'refresh', child: Text('Metadaten aktualisieren')),
              if (admin) const PopupMenuItem(value: 'analyze', child: Text('Analysieren')),
              const PopupMenuItem(value: 'all', child: Text('Alle auswählen')),
            ],
          ),
        ]),
      ),
    );
  }

  @override
  void dispose() {
    scroll.dispose();
    search.dispose();
    from.dispose();
    to.dispose();
    super.dispose();
  }

  Map<String, String> get q => {
        if (type.isNotEmpty) 'type': type,
        if (from.text.trim().isNotEmpty) 'year>': from.text.trim(),
        if (to.text.trim().isNotEmpty) 'year<': to.text.trim(),
        if (search.text.trim().isNotEmpty) 'q': search.text.trim(),
        if (container.isNotEmpty) 'container': container,
        ...filters,
        'sort': '${sortField == 'show' ? 'episode_order' : sortField}:${desc ? 'desc' : 'asc'}',
      };

  Future<void> reload() async {
    final my = ++gen;
    setState(() {
      items.clear();
      fetched = 0;
      total = 0;
      error = null;
      loading = true;
    });
    try {
      final r = await widget.api.list(sectionId, q, size: page);
      if (my != gen || !mounted) return;
      setState(() {
        _add(r.items);
        total = r.total;
        loading = false;
        letters = [];
      });
      loadLetters(my);
    } catch (e) {
      if (my == gen && mounted) setState(() {
        error = '$e';
        loading = false;
      });
    }
  }

  /// The letters at the side (only when sorted by title): each with the position of its first poster.
  Future<void> loadLetters(int my) async {
    if (sortField != 'title' || total <= page ~/ 2) return;
    try {
      final dirs = await widget.api.firstCharacter(sectionId, q);
      if (my != gen || !mounted) return;
      var sum = 0;
      final l = <({String key, int offset})>[];
      for (final d in dirs) {
        final before = sum;
        sum += d.size;
        l.add((key: d.key, offset: desc ? total - before - d.size : before));
      }
      setState(() => letters = l);
    } catch (_) {}
  }

  /// Loads the pages up to the letter's first poster and scrolls there; nothing is filtered.
  Future<void> jumpTo(int offset) async {
    if (jumping) return;
    jumping = true;
    try {
      final my = gen;
      while (items.length <= offset && fetched < total && my == gen && mounted && error == null) {
        if (loading) {
          await Future<void>.delayed(const Duration(milliseconds: 40));
        } else {
          await loadMore();
        }
      }
      if (!mounted || !scroll.hasClients || gridWidth <= 0) return;
      const maxExtent = 170.0, spacing = 12.0, aspect = 0.48;
      final inner = gridWidth - 16 - (letters.isEmpty ? 16 : 32);
      final cols = (inner / (maxExtent + spacing)).ceil().clamp(1, 1000);
      final w = (inner - (cols - 1) * spacing) / cols;
      final rowH = w / aspect + spacing;
      final target = (offset ~/ cols) * rowH;
      scroll.animateTo(target.clamp(0.0, scroll.position.maxScrollExtent), duration: const Duration(milliseconds: 350), curve: Curves.easeOut);
    } finally {
      jumping = false;
    }
  }

  /// Adds a page. While the server is changing titles (a scan, updating details) the sorting can move
  /// a title from one page to the next, so one that is already shown is left out.
  void _add(List<Map<String, dynamic>> page) {
    fetched += page.length;
    if (page.isEmpty) total = fetched; // the library got smaller meanwhile: nothing more to load
    final seen = {for (final i in items) '${i['id'] ?? i['ratingKey']}'};
    for (final i in page) {
      if (seen.add('${i['id'] ?? i['ratingKey']}')) items.add(i);
    }
  }

  Future<void> loadMore() async {
    if (loading || fetched >= total) return;
    final my = gen;
    setState(() => loading = true);
    try {
      final r = await widget.api.list(sectionId, q, start: fetched, size: page);
      if (my != gen || !mounted) return;
      setState(() {
        _add(r.items);
        loading = false;
      });
    } catch (e) {
      if (my == gen && mounted) setState(() {
        error = '$e';
        loading = false;
      });
    }
  }

  void setType(String t) {
    type = t;
    sortField = t == 'season' || t == 'episode' ? 'show' : 'title';
    desc = false;
    filters.clear(); // the kinds of filter differ per type
    available = null;
    reload();
  }

  void setSort(String f) {
    sortField = f;
    desc = !_sortAsc.contains(f);
    reload();
  }

  Future<void> openFilters() async {
    try {
      available ??= await widget.api.filters(sectionId, type: type);
    } catch (e) {
      if (mounted) ScaffoldMessenger.of(context).showSnackBar(SnackBar(content: Text('$e')));
      return;
    }
    if (!mounted) return;
    await showDialog(
      context: context,
      builder: (_) => _FilterDialog(
        available: available!,
        filters: filters,
        onChanged: reload,
      ),
    );
    if (mounted) setState(() {});
  }

  @override
  Widget build(BuildContext context) {
    final sortFields = {if (type == 'season' || type == 'episode') 'show': 'Serie', ..._sortFields};
    final types = switch (sectionType) {
      'show' => const {'': 'Serien', 'season': 'Staffeln', 'episode': 'Folgen'},
      'music' || 'artist' => const {'': 'Künstler', 'album': 'Alben', 'track': 'Titel'},
      _ => null,
    };
    final nActive = filters.length;
    final bar = Padding(
      padding: const EdgeInsets.fromLTRB(16, 8, 16, 0),
      child: Wrap(spacing: 8, runSpacing: 8, crossAxisAlignment: WrapCrossAlignment.center, children: [
        if (types != null)
          SegmentedButton<String>(
            showSelectedIcon: false,
            segments: [for (final e in types.entries) ButtonSegment(value: e.key, label: Text(e.value))],
            selected: {type},
            onSelectionChanged: (s) => setType(s.first),
          ),
        OutlinedButton.icon(onPressed: openFilters, icon: const Icon(Icons.filter_list), label: Text(nActive > 0 ? 'Filter ($nActive)' : 'Filter')),
        PopupMenuButton<String>(
          tooltip: 'Sortieren',
          onSelected: setSort,
          itemBuilder: (_) => [for (final e in sortFields.entries) CheckedPopupMenuItem(value: e.key, checked: e.key == sortField, child: Text(e.value))],
          child: Chip(avatar: const Icon(Icons.sort, size: 18), label: Text(sortFields[sortField] ?? sortField)),
        ),
        IconButton(
          tooltip: desc ? 'Absteigend' : 'Aufsteigend',
          onPressed: () {
            desc = !desc;
            reload();
          },
          icon: Icon(desc ? Icons.arrow_downward : Icons.arrow_upward),
        ),
        SizedBox(width: 170, child: _field(search, 'Nach Titel filtern', icon: Icons.search)),
        SizedBox(width: 90, child: _field(from, 'Jahr ab', number: true)),
        SizedBox(width: 90, child: _field(to, 'Jahr bis', number: true)),
        PopupMenuButton<String>(
          tooltip: 'Dateityp',
          onSelected: (v) {
            container = v;
            reload();
          },
          itemBuilder: (_) => [
            CheckedPopupMenuItem(value: '', checked: container.isEmpty, child: const Text('Alle Dateitypen')),
            for (final c in _containers) CheckedPopupMenuItem(value: c, checked: container == c, child: Text(c.toUpperCase())),
          ],
          child: Chip(avatar: const Icon(Icons.insert_drive_file_outlined, size: 18), label: Text(container.isEmpty ? 'Dateityp' : container.toUpperCase())),
        ),
        if (loading || total > 0 || error == null) Text(total == 1 ? '1 Eintrag' : '$total Einträge', style: const TextStyle(color: Colors.white60)),
      ]),
    );
    final chips = nActive == 0
        ? null
        : Padding(
            padding: const EdgeInsets.fromLTRB(16, 8, 16, 0),
            child: Wrap(spacing: 8, runSpacing: 4, children: [
              for (final e in filters.entries)
                InputChip(
                  label: Text(e.value == 'true' ? _filterName(e.key) : e.value == 'false' ? '${_filterName(e.key)}: nein' : '${_filterName(e.key)}: ${_valueName(e.key, e.value)}'),
                  onDeleted: () {
                    filters.remove(e.key);
                    reload();
                  },
                ),
              if (nActive > 1)
                TextButton(
                  onPressed: () {
                    filters.clear();
                    reload();
                  },
                  child: const Text('Filter löschen'),
                ),
            ]),
          );
    return Scaffold(
      appBar: AppBar(title: Text('${widget.section['title']}'), actions: [
        if (sectionType == 'movie' || sectionType == 'show') ...[
          TextButton.icon(
            icon: const Icon(Icons.collections_bookmark_outlined),
            label: const Text('Sammlungen'),
            onPressed: () => Navigator.push(context, MaterialPageRoute(builder: (_) => GroupsScreen(api: widget.api, section: widget.section, kind: 'collections'))),
          ),
          TextButton.icon(
            icon: const Icon(Icons.category_outlined),
            label: const Text('Kategorien'),
            onPressed: () => Navigator.push(context, MaterialPageRoute(builder: (_) => GroupsScreen(api: widget.api, section: widget.section, kind: 'categories'))),
          ),
        ],
        TextButton.icon(
          icon: const Icon(Icons.playlist_play),
          label: const Text('Playlists'),
          onPressed: () => Navigator.push(context, MaterialPageRoute(builder: (_) => PlaylistsScreen(api: widget.api, section: '${widget.section['key']}'))),
        ),
      ]),
      bottomNavigationBar: selectionBar(),
      body: Column(children: [
        bar,
        if (chips != null) chips,
        Expanded(
          child: error != null && items.isEmpty
              ? Center(child: Text(error!))
              : items.isEmpty && !loading
                  ? const Center(child: Text('Nichts passt zu diesen Filtern.'))
                  : LayoutBuilder(builder: (context, box) {
                      gridWidth = box.maxWidth;
                      return Stack(children: [
                        GridView.builder(
                          controller: scroll,
                          padding: EdgeInsets.fromLTRB(16, 16, letters.isEmpty ? 16 : 32, 16),
                          gridDelegate: const SliverGridDelegateWithMaxCrossAxisExtent(maxCrossAxisExtent: 170, childAspectRatio: 0.48, crossAxisSpacing: 12, mainAxisSpacing: 12),
                          itemCount: items.length + (loading ? 1 : 0),
                          itemBuilder: (_, i) => i >= items.length
                              ? const Center(child: CircularProgressIndicator())
                              : Poster(api: widget.api, item: items[i], width: 170, onTap: () => openItem(context, widget.api, items[i]), onPick: () => toggle(items[i]), picked: picked.contains(_id(items[i])), selecting: picked.isNotEmpty),
                        ),
                        if (letters.isNotEmpty)
                          Positioned(
                            right: 2,
                            top: 8,
                            bottom: 8,
                            width: 22,
                            child: Column(mainAxisAlignment: MainAxisAlignment.center, children: [
                              for (final l in letters)
                                Flexible(
                                  child: ConstrainedBox(
                                    constraints: const BoxConstraints(maxHeight: 20),
                                    child: InkWell(
                                      onTap: () => jumpTo(l.offset),
                                      child: Center(child: FittedBox(child: Text(l.key, style: const TextStyle(fontSize: 11, color: Colors.white60)))),
                                    ),
                                  ),
                                ),
                            ]),
                          ),
                      ]);
                    }),
        ),
      ]),
    );
  }

  Widget _field(TextEditingController c, String hint, {IconData? icon, bool number = false}) => TextField(
        controller: c,
        keyboardType: number ? TextInputType.number : TextInputType.text,
        decoration: InputDecoration(isDense: true, hintText: hint, prefixIcon: icon == null ? null : Icon(icon, size: 18), border: const OutlineInputBorder()),
        onSubmitted: (_) => reload(),
        onTapOutside: (_) {
          FocusManager.instance.primaryFocus?.unfocus();
        },
      );
}

/// The filter menu: the kinds of filter on the left (states first, then the lists of values), the values of the
/// chosen one with a search box on the right. Every choice applies at once, like on the web.
class _FilterDialog extends StatefulWidget {
  final List<Map<String, dynamic>> available;
  final Map<String, String> filters;
  final VoidCallback onChanged;
  const _FilterDialog({required this.available, required this.filters, required this.onChanged});
  @override
  State<_FilterDialog> createState() => _FilterDialogState();
}

class _FilterDialogState extends State<_FilterDialog> {
  String cur = '';
  final needle = TextEditingController();

  late final byName = {for (final f in widget.available) '${f['filter']}': f};
  late final states = [for (final k in _groupState) if (byName[k] != null) byName[k]!];
  late final main = [for (final k in _groupMain) if (byName[k] != null) byName[k]!];
  late final more = [for (final f in widget.available) if (!states.contains(f) && !main.contains(f)) f];

  @override
  void initState() {
    super.initState();
    final first = [...main, ...more].where((f) => f['type'] != 'boolean').firstOrNull;
    cur = first == null ? '' : '${first['filter']}';
  }

  @override
  void dispose() {
    needle.dispose();
    super.dispose();
  }

  void set(String f, String? v) {
    setState(() {
      if (v == null || v.isEmpty) {
        widget.filters.remove(f);
      } else {
        widget.filters[f] = v;
      }
    });
    widget.onChanged();
  }

  Widget entry(Map<String, dynamic> f) {
    final name = '${f['filter']}';
    final val = widget.filters[name] ?? '';
    if (f['type'] == 'boolean') {
      return ListTile(
        dense: true,
        selected: val.isNotEmpty,
        title: Text(_filterName(name)),
        trailing: val.isNotEmpty ? const Icon(Icons.check, size: 18) : (f['count'] != null ? Text('${f['count']}', style: const TextStyle(color: Colors.white54)) : null),
        onTap: () => set(name, val.isNotEmpty ? null : 'true'),
      );
    }
    return ListTile(
      dense: true,
      selected: name == cur,
      title: Text(_filterName(name), style: TextStyle(fontWeight: val.isNotEmpty ? FontWeight.bold : null)),
      trailing: val.isNotEmpty ? Text(_valueName(name, val), style: const TextStyle(fontSize: 12, color: Colors.white54)) : const Icon(Icons.chevron_right, size: 18),
      onTap: () => setState(() {
        cur = name;
        needle.clear();
      }),
    );
  }

  @override
  Widget build(BuildContext context) {
    final f = byName[cur];
    final on = widget.filters[cur] ?? '';
    final n = needle.text.trim().toLowerCase();
    final values = f == null ? const <Map>[] : (f['values'] as List? ?? const []).cast<Map>().where((v) => n.isEmpty || _valueName(cur, '${v['value']}').toLowerCase().contains(n)).toList();
    final size = MediaQuery.of(context).size;
    return Dialog(
      child: SizedBox(
        width: size.width < 680 ? size.width - 48 : 620,
        height: size.height < 560 ? size.height - 48 : 480,
        child: Row(children: [
          SizedBox(
            width: 210,
            child: ListView(children: [
              for (final g in [states, main, more])
                if (g.isNotEmpty) ...[...g.map(entry), const Divider(height: 1)],
            ]),
          ),
          const VerticalDivider(width: 1),
          Expanded(
            child: f == null
                ? const Center(child: Text('Keine gefunden.'))
                : Column(children: [
                    Padding(
                      padding: const EdgeInsets.all(8),
                      child: TextField(
                        controller: needle,
                        onChanged: (_) => setState(() {}),
                        decoration: InputDecoration(isDense: true, hintText: '${_filterName(cur)} suchen …', prefixIcon: const Icon(Icons.search, size: 18), border: const OutlineInputBorder()),
                      ),
                    ),
                    Expanded(
                      child: values.isEmpty
                          ? const Center(child: Text('Keine gefunden.'))
                          : ListView.builder(
                              itemCount: values.length,
                              itemBuilder: (_, i) {
                                final v = '${values[i]['value']}';
                                return ListTile(
                                  dense: true,
                                  selected: v == on,
                                  title: Text(_valueName(cur, v)),
                                  trailing: Text('${values[i]['count'] ?? ''}', style: const TextStyle(color: Colors.white54)),
                                  onTap: () => set(cur, v == on ? null : v),
                                );
                              },
                            ),
                    ),
                  ]),
          ),
        ]),
      ),
    );
  }
}
