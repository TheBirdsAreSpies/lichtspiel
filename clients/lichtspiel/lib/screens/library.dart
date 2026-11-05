import 'package:flutter/material.dart';
import '../api.dart';
import '../widgets/poster.dart';
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
  int gen = 0; // bumped on every change so a slow answer to an old question is dropped
  bool loading = false;
  String? error;
  List<Map<String, dynamic>>? available; // the kinds of filter and their values, loaded when first opened

  String get sectionId => '${widget.section['key']}';
  String get sectionType => '${widget.section['type']}';
  bool get done => items.length >= total && total > 0 || (total == 0 && !loading && error == null && gen > 0 && items.isEmpty);

  @override
  void initState() {
    super.initState();
    filters.addAll(widget.initialFilters);
    scroll.addListener(() {
      if (scroll.hasClients && scroll.position.pixels > scroll.position.maxScrollExtent - 600) loadMore();
    });
    reload();
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
      total = 0;
      error = null;
      loading = true;
    });
    try {
      final r = await widget.api.list(sectionId, q, size: page);
      if (my != gen || !mounted) return;
      setState(() {
        items.addAll(r.items);
        total = r.total;
        loading = false;
      });
    } catch (e) {
      if (my == gen && mounted) setState(() {
        error = '$e';
        loading = false;
      });
    }
  }

  Future<void> loadMore() async {
    if (loading || items.length >= total) return;
    final my = gen;
    setState(() => loading = true);
    try {
      final r = await widget.api.list(sectionId, q, start: items.length, size: page);
      if (my != gen || !mounted) return;
      setState(() {
        items.addAll(r.items);
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
      body: Column(children: [
        bar,
        if (chips != null) chips,
        Expanded(
          child: error != null && items.isEmpty
              ? Center(child: Text(error!))
              : items.isEmpty && !loading
                  ? const Center(child: Text('Nichts passt zu diesen Filtern.'))
                  : GridView.builder(
                      controller: scroll,
                      padding: const EdgeInsets.all(16),
                      gridDelegate: const SliverGridDelegateWithMaxCrossAxisExtent(maxCrossAxisExtent: 170, childAspectRatio: 0.48, crossAxisSpacing: 12, mainAxisSpacing: 12),
                      itemCount: items.length + (loading ? 1 : 0),
                      itemBuilder: (_, i) => i >= items.length
                          ? const Center(child: CircularProgressIndicator())
                          : Poster(api: widget.api, item: items[i], width: 170, onTap: () => openItem(context, widget.api, items[i])),
                    ),
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
