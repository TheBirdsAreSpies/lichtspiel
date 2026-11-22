import 'dart:math';
import 'package:flutter/material.dart';
import '../api.dart';
import '../widgets/pick.dart';
import '../widgets/poster.dart';
import 'edit.dart';
import 'downloads.dart';
import 'player.dart';
import 'home.dart';
import 'playlists.dart';
import '../widgets/bar.dart';
import '../theme.dart';
import '../widgets/states.dart';

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

String filterName(String f) => _filterNames[f] ?? f;
String valueName(String f, String v) => f == 'video_resolution' ? (_resNames[v] ?? v) : f == 'decade' ? '${v}er' : v;

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
  Object? error;
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
      say(errorText(e));
    }
  }

  /// Runs [f] for the chosen ones, then tells what happened; [keep] leaves the selection in place.
  /// With [undo] the message offers "Rückgängig", which runs it for the same titles.
  Future<void> doChosen(Future<void> Function(List<String> ids) f, String done, {bool keep = false, Future<void> Function()? undo}) async {
    final ids = picked.toList();
    try {
      await f(ids);
      if (undo != null && mounted) {
        undoSnack(context, done, () async {
          try {
            await undo();
            await refresh(ids);
          } catch (e) {
            say(errorText(e));
          }
        });
      } else {
        say(done);
      }
      await refresh(ids);
      if (!keep) picked.clear();
      if (mounted) setState(() {});
    } catch (e) {
      say(errorText(e));
    }
  }

  /// Fetches the titles again, so their posters show the change.
  Future<void> refresh(List<String> ids) async {
    for (final id in ids) {
      final at = items.indexWhere((i) => _id(i) == id);
      if (at >= 0) items[at].addAll(await widget.api.item(id));
    }
    if (mounted) setState(() {});
  }

  /// Marks the chosen ones watched or not; undoing gives each one back the state it had, as long as
  /// none of them was half watched (a show with some episodes seen), which a single mark cannot restore.
  Future<void> markChosen(bool on) {
    num n(Map i, String k) => (i[k] ?? 0) as num;
    final list = chosen;
    final partial = list.any((i) => n(i, 'leafCount') > 0 && n(i, 'viewedLeafCount') > 0 && n(i, 'viewedLeafCount') < n(i, 'leafCount'));
    bool seen(Map i) => n(i, 'leafCount') > 0 ? n(i, 'viewedLeafCount') >= n(i, 'leafCount') : n(i, 'viewCount') > 0;
    final wasOn = [for (final i in list) if (seen(i)) _id(i)], wasOff = [for (final i in list) if (!seen(i)) _id(i)];
    return doChosen((ids) => widget.api.setWatchedAll(ids, on), on ? 'Als gesehen markiert' : 'Als ungesehen markiert', undo: partial ? null : () async {
      if (wasOn.isNotEmpty) await widget.api.setWatchedAll(wasOn, true);
      if (wasOff.isNotEmpty) await widget.api.setWatchedAll(wasOff, false);
    });
  }

  /// Several movies (or episodes) become one title with several versions; the chosen main one stays visible.
  Future<void> mergeChosen() async {
    final list = chosen;
    if (list.length < 2) return;
    var main = _id(list.first);
    String name(Map<String, dynamic> i) => i['type'] == 'episode' ? 'S${i['parentIndex'] ?? 0}E${i['index'] ?? 0} ${i['title']}' : '${i['title']}${i['year'] != null ? ' (${i['year']})' : ''}';
    final ok = await showDialog<bool>(
      context: context,
      builder: (ctx) => StatefulBuilder(
        builder: (ctx, set) => AlertDialog(
          title: const Text('Zu einem Titel zusammenführen'),
          content: SizedBox(
            width: 420,
            child: Column(mainAxisSize: MainAxisSize.min, crossAxisAlignment: CrossAxisAlignment.start, children: [
              Text('Die anderen erscheinen danach als Versionen dieses Titels. Fortschritt und „gesehen“ zählen für den Titel.', style: TextStyle(color: context.faint)),
              const SizedBox(height: 8),
              const Text('Hauptversion:'),
              for (final i in list)
                ListTile(
                  dense: true,
                  contentPadding: EdgeInsets.zero,
                  leading: Icon(main == _id(i) ? Icons.radio_button_checked : Icons.radio_button_off),
                  title: Text(name(i), maxLines: 1, overflow: TextOverflow.ellipsis),
                  onTap: () => set(() => main = _id(i)),
                ),
            ]),
          ),
          actions: [
            TextButton(onPressed: () => Navigator.pop(ctx, false), child: const Text('Abbrechen')),
            FilledButton(onPressed: () => Navigator.pop(ctx, true), child: const Text('Zusammenführen')),
          ],
        ),
      ),
    );
    if (ok != true) return;
    final ids = [main, for (final i in list) if (_id(i) != main) _id(i)];
    try {
      await widget.api.post('/api/merge', {'ids': ids});
      say('${ids.length} zu einem Titel zusammengeführt');
      picked.clear();
      await reload();
    } catch (e) {
      say(errorText(e));
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
          TextButton(
            onPressed: () {
              final added = [for (final i in chosen) if (i['watchlist'] != true) _id(i)];
              doChosen((ids) => widget.api.setWatchlistAll(ids, true), 'Auf die Merkliste gesetzt', undo: added.isEmpty ? null : () => widget.api.setWatchlistAll(added, false));
            },
            child: const Text('+ Merkliste'),
          ),
          TextButton(onPressed: () => markChosen(true), child: const Text('Gesehen')),
          TextButton(onPressed: () => markChosen(false), child: const Text('Ungesehen')),
          TextButton(onPressed: () => addToPlaylistDialog(context, widget.api, picked.first, ids: picked.toList()), child: const Text('Zur Playlist')),
          if (admin && one) TextButton(onPressed: () async {
            final Map<String, dynamic> it;
            try {
              it = await widget.api.item(picked.first);
            } catch (e) {
              return say(errorText(e));
            }
            if (!mounted) return;
            await editDetailsDialog(context, widget.api, it);
            await doChosen((_) async {}, 'Gespeichert', keep: true);
          }, child: const Text('Bearbeiten')),
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
                case 'download':
                  downloadDialog(context, widget.api, chosen);
                case 'merge':
                  mergeChosen();
                case 'all':
                  setState(() => picked.addAll(items.map(_id)));
              }
            },
            itemBuilder: (_) => [
              const PopupMenuItem(value: 'shuffle', child: Text('Zufallswiedergabe')),
              if (admin) const PopupMenuItem(value: 'refresh', child: Text('Metadaten aktualisieren')),
              if (admin) const PopupMenuItem(value: 'analyze', child: Text('Analysieren')),
              if (admin && picked.length > 1 && chosen.every((i) => i['type'] == chosen.first['type'] && (i['type'] == 'movie' || i['type'] == 'episode')))
                const PopupMenuItem(value: 'merge', child: Text('Zu einem Titel zusammenführen…')),
              if (DownloadRights.allowed) const PopupMenuItem(value: 'download', child: Text('Herunterladen…')),
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
        error = e;
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
        error = e;
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

  /// What a poster says under its title, by the way the library is sorted (like the web app).
  String? sortCaption(Map<String, dynamic> it) {
    String dur(num ms) {
      final m = (ms / 60000).round();
      if (m < 1) return '${(ms / 1000).round().clamp(1, 59)} s';
      return m >= 60 ? '${m ~/ 60} h ${m % 60} min' : '$m min';
    }

    String rel(num sec) {
      final d = ((DateTime.now().millisecondsSinceEpoch / 1000 - sec) / 86400).round();
      String n(int v, String one, String many) => v == 1 ? 'vor 1 $one' : 'vor $v $many';
      if (d < 1) return 'heute';
      if (d < 14) return n(d, 'Tag', 'Tagen');
      if (d < 60) return n((d / 7).round(), 'Woche', 'Wochen');
      if (d < 730) return n((d / 30.4).round(), 'Monat', 'Monaten');
      return n((d / 365).round(), 'Jahr', 'Jahren');
    }

    String num1(num v) => v.toStringAsFixed(1).replaceAll('.', ',');
    final scores = it['scores'] is Map ? it['scores'] as Map : const {};
    final media = (it['Media'] is List && (it['Media'] as List).isNotEmpty && (it['Media'] as List).first is Map) ? (it['Media'] as List).first as Map : const {};
    const src = {'imdb': 'IMDb', 'tmdb': 'TMDB', 'rotten_tomatoes': 'RT', 'metacritic': 'MC'};
    String? score(List<String> keys) {
      for (final k in keys) {
        if (scores[k] != null && '${scores[k]}'.isNotEmpty) return '${src[k]} ${'${scores[k]}'.replaceAll('/10', '')}';
      }
      return null;
    }

    num? n(String k) => it[k] is num ? it[k] as num : null;
    switch (sortField) {
      case 'duration':
        return n('duration') != null && n('duration')! > 0 ? dur(n('duration')!) : null;
      case 'release_date':
        final d = DateTime.tryParse('${it['originallyAvailableAt'] ?? ''}');
        return d != null ? '${d.day.toString().padLeft(2, '0')}.${d.month.toString().padLeft(2, '0')}.${d.year}' : '${it['year'] ?? ''}';
      case 'audience_rating':
        return score(['imdb', 'tmdb']) ?? (n('rating') != null && n('rating')! > 0 ? num1(n('rating')!) : null);
      case 'critic_rating':
        return score(['rotten_tomatoes', 'metacritic']);
      case 'user_rating':
        return n('userRating') != null && n('userRating')! > 0 ? '★' * (n('userRating')! / 2).round() : null;
      case 'content_rating':
        return it['contentRating']?.toString();
      case 'progress':
        return n('viewOffset') != null && n('viewOffset')! > 0 && n('duration') != null && n('duration')! > 0 ? '${(100 * n('viewOffset')! / n('duration')!).round()} %' : null;
      case 'view_count':
        return n('viewCount') != null && n('viewCount')! > 0 ? '${n('viewCount')!.toInt()}× gesehen' : null;
      case 'added_at':
        return n('addedAt') != null && n('addedAt')! > 0 ? rel(n('addedAt')!) : null;
      case 'last_viewed_at':
        return n('lastViewedAt') != null && n('lastViewedAt')! > 0 ? rel(n('lastViewedAt')!) : null;
      case 'resolution':
        final h = media['height'];
        return h is num && h > 0 ? (h >= 2000 ? '4K' : '${h.toInt()}p') : null;
      case 'bitrate':
        final b = media['bitrate'];
        return b is num && b > 0 ? '${num1(b / 1000)} Mbit/s' : null;
    }
    return null;
  }

  /// The dice: one title at random from what the filters leave.
  Future<void> randomPick() async {
    if (total < 1) return;
    try {
      final r = await widget.api.list(sectionId, q, start: Random().nextInt(total), size: 1);
      if (!mounted || r.items.isEmpty) return;
      await randomDialog(context, widget.api, r.items.first, randomPick);
    } catch (e) {
      say(errorText(e));
    }
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
      if (mounted) ScaffoldMessenger.of(context).showSnackBar(SnackBar(content: Text(errorText(e))));
      return;
    }
    if (!mounted) return;
    await showFilters(context, available: available!, filters: filters, onChanged: reload);
    if (mounted) setState(() {});
  }

  /// Phones: the year range and the file type, which have no room in the bar.
  Future<void> openMore() async {
    await showModalBottomSheet(
      context: context,
      isScrollControlled: true,
      useSafeArea: true,
      showDragHandle: true,
      builder: (ctx) => StatefulBuilder(
        builder: (ctx, set) => Padding(
          padding: EdgeInsets.fromLTRB(16, 0, 16, 16 + MediaQuery.viewInsetsOf(ctx).bottom),
          child: Column(mainAxisSize: MainAxisSize.min, crossAxisAlignment: CrossAxisAlignment.start, children: [
            Text('Jahr', style: Theme.of(ctx).textTheme.titleSmall),
            const SizedBox(height: 8),
            Row(children: [
              Expanded(child: _field(from, 'ab', number: true)),
              const SizedBox(width: 12),
              Expanded(child: _field(to, 'bis', number: true)),
            ]),
            const SizedBox(height: 16),
            Text('Dateityp', style: Theme.of(ctx).textTheme.titleSmall),
            const SizedBox(height: 8),
            Wrap(spacing: 8, runSpacing: 4, children: [
              for (final c in ['', ..._containers])
                ChoiceChip(
                  label: Text(c.isEmpty ? 'Alle' : c.toUpperCase()),
                  selected: container == c,
                  onSelected: (_) => set(() => container = c),
                ),
            ]),
            const SizedBox(height: 16),
            Row(children: [
              TextButton(
                onPressed: () => set(() {
                  from.clear();
                  to.clear();
                  container = '';
                }),
                child: const Text('Zurücksetzen'),
              ),
              const Spacer(),
              FilledButton(onPressed: () => Navigator.pop(ctx), child: const Text('Anwenden')),
            ]),
          ]),
        ),
      ),
    );
    if (mounted) reload();
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
    final narrow = isNarrow(context);
    final nMore = (from.text.trim().isNotEmpty || to.text.trim().isNotEmpty ? 1 : 0) + (container.isNotEmpty ? 1 : 0);
    final countText = total == 1 ? '1 Eintrag' : '$total Einträge';
    final compact = Padding(
      padding: const EdgeInsets.fromLTRB(12, 8, 4, 0),
      child: Column(children: [
        if (types != null)
          SizedBox(
            width: double.infinity,
            child: SegmentedButton<String>(
              showSelectedIcon: false,
              segments: [for (final e in types.entries) ButtonSegment(value: e.key, label: Text(e.value))],
              selected: {type},
              onSelectionChanged: (s) => setType(s.first),
            ),
          ),
        if (types != null) const SizedBox(height: 8),
        Row(children: [
          Expanded(child: _field(search, 'Nach Titel filtern', icon: Icons.search)),
          IconButton(tooltip: 'Filter', onPressed: openFilters, icon: Badge(isLabelVisible: nActive > 0, label: Text('$nActive'), child: const Icon(Icons.filter_list))),
          PopupMenuButton<String>(
            tooltip: 'Sortieren: ${sortFields[sortField] ?? sortField}',
            icon: const Icon(Icons.sort),
            onSelected: (v) {
              if (v == '<asc>' || v == '<desc>') {
                desc = v == '<desc>';
                reload();
              } else {
                setSort(v);
              }
            },
            itemBuilder: (_) => [
              for (final e in sortFields.entries) CheckedPopupMenuItem(value: e.key, checked: e.key == sortField, child: Text(e.value)),
              const PopupMenuDivider(),
              CheckedPopupMenuItem(value: '<asc>', checked: !desc, child: const Text('Aufsteigend')),
              CheckedPopupMenuItem(value: '<desc>', checked: desc, child: const Text('Absteigend')),
            ],
          ),
          if (total > 1) IconButton(tooltip: 'Zufällig einen auswählen', onPressed: randomPick, icon: const Icon(Icons.casino_outlined)),
          IconButton(tooltip: 'Jahr und Dateityp', onPressed: openMore,icon: Badge(isLabelVisible: nMore > 0, label: Text('$nMore'), child: const Icon(Icons.tune))),
        ]),
      ]),
    );
    final bar = narrow ? compact : Padding(
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
        if (total > 1) IconButton(tooltip: 'Zufällig einen auswählen', onPressed: randomPick, icon: const Icon(Icons.casino_outlined)),
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
        if (loading || total > 0 || error == null) Text(countText, style: TextStyle(color: context.dim)),
      ]),
    );
    final chips = nActive == 0
        ? null
        : Padding(
            padding: const EdgeInsets.fromLTRB(16, 8, 16, 0),
            child: Wrap(spacing: 8, runSpacing: 4, children: [
              for (final e in filters.entries)
                InputChip(
                  label: Text(e.value == 'true' ? filterName(e.key) : e.value == 'false' ? '${filterName(e.key)}: nein' : '${filterName(e.key)}: ${valueName(e.key, e.value)}'),
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
      appBar: AppBar(
          title: narrow && total > 0
              ? Column(crossAxisAlignment: CrossAxisAlignment.start, children: [
                  Text('${widget.section['title']}'),
                  Text('${sortFields[sortField] ?? sortField} · $countText', style: TextStyle(fontSize: 12, color: context.dim)),
                ])
              : Text('${widget.section['title']}'),
          actions: barActions(context, labels: true, [
        if (sectionType == 'movie' || sectionType == 'show') ...[
          BarAction(Icons.collections_bookmark_outlined, 'Sammlungen', () => Navigator.push(context, MaterialPageRoute(builder: (_) => GroupsScreen(api: widget.api, section: widget.section, kind: 'collections')))),
          BarAction(Icons.category_outlined, 'Kategorien', () => Navigator.push(context, MaterialPageRoute(builder: (_) => GroupsScreen(api: widget.api, section: widget.section, kind: 'categories')))),
        ],
        BarAction(Icons.playlist_play, 'Playlists', () => Navigator.push(context, MaterialPageRoute(builder: (_) => PlaylistsScreen(api: widget.api, section: '${widget.section['key']}')))),
      ])),
      bottomNavigationBar: selectionBar(),
      body: Column(children: [
        bar,
        if (chips != null) chips,
        Expanded(
          child: error != null && items.isEmpty
              ? ErrorView(error: error!, onRetry: reload)
              : items.isEmpty && loading
                  ? const GridSkeleton()
              : items.isEmpty && !loading
                  ? ErrorView(
                      error: nActive + nMore > 0 || search.text.trim().isNotEmpty ? 'Nichts passt zu diesen Filtern.' : 'Diese Bibliothek ist noch leer.',
                    )
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
                              : Poster(api: widget.api, item: items[i], width: 170, caption: sortCaption(items[i]), onTap: () => openItem(context, widget.api, items[i]), onPick: () => toggle(items[i]), picked: picked.contains(_id(items[i])), selecting: picked.isNotEmpty),
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
                                      child: Center(child: FittedBox(child: Text(l.key, style: TextStyle(fontSize: 11, color: context.dim)))),
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

/// Opens the filter menu: a dialog with two columns on a computer, a sheet from the bottom on a phone.
Future<void> showFilters(BuildContext context, {required List<Map<String, dynamic>> available, required Map<String, String> filters, required VoidCallback onChanged}) {
  if (!isNarrow(context)) {
    return showDialog(context: context, builder: (_) => FilterDialog(available: available, filters: filters, onChanged: onChanged));
  }
  return showModalBottomSheet(
    context: context,
    isScrollControlled: true,
    useSafeArea: true,
    showDragHandle: true,
    builder: (_) => FractionallySizedBox(heightFactor: .85, child: FilterDialog(available: available, filters: filters, onChanged: onChanged, sheet: true)),
  );
}

/// The filter menu: the kinds of filter on the left (states first, then the lists of values), the values of the
/// chosen one with a search box on the right. Every choice applies at once, like on the web.
class FilterDialog extends StatefulWidget {
  final List<Map<String, dynamic>> available;
  final Map<String, String> filters;
  final VoidCallback onChanged;
  /// On a phone: one column, first the kinds of filter, then the values of the chosen one.
  final bool sheet;
  const FilterDialog({super.key, required this.available, required this.filters, required this.onChanged, this.sheet = false});
  @override
  State<FilterDialog> createState() => FilterDialogState();
}

class FilterDialogState extends State<FilterDialog> {
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
    cur = first == null || widget.sheet ? '' : '${first['filter']}';
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
        title: Text(filterName(name)),
        trailing: val.isNotEmpty ? const Icon(Icons.check, size: 18) : (f['count'] != null ? Text('${f['count']}', style: TextStyle(color: context.faint)) : null),
        onTap: () => set(name, val.isNotEmpty ? null : 'true'),
      );
    }
    return ListTile(
      dense: true,
      selected: name == cur,
      title: Text(filterName(name), style: TextStyle(fontWeight: val.isNotEmpty ? FontWeight.bold : null)),
      trailing: val.isNotEmpty ? Text(valueName(name, val), style: TextStyle(fontSize: 12, color: context.faint)) : const Icon(Icons.chevron_right, size: 18),
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
    final values = f == null ? const <Map>[] : (f['values'] as List? ?? const []).cast<Map>().where((v) => n.isEmpty || valueName(cur, '${v['value']}').toLowerCase().contains(n)).toList();
    final kinds = ListView(children: [
      for (final g in [states, main, more])
        if (g.isNotEmpty) ...[...g.map(entry), const Divider(height: 1)],
    ]);
    if (widget.sheet) {
      return Column(children: [
        Padding(
          padding: const EdgeInsets.fromLTRB(4, 0, 12, 4),
          child: Row(children: [
            if (f != null) IconButton(tooltip: 'Zurück', icon: const Icon(Icons.arrow_back), onPressed: () => setState(() => cur = '')) else const SizedBox(width: 12),
            Expanded(child: Text(f != null ? filterName(cur) : 'Filter', style: Theme.of(context).textTheme.titleMedium)),
            if (widget.filters.isNotEmpty && f == null)
              TextButton(
                onPressed: () {
                  setState(widget.filters.clear);
                  widget.onChanged();
                },
                child: const Text('Alle löschen'),
              ),
            TextButton(onPressed: () => Navigator.pop(context), child: const Text('Fertig')),
          ]),
        ),
        const Divider(height: 1),
        Expanded(child: f == null ? kinds : valueList(f, on, values)),
      ]);
    }
    final size = MediaQuery.of(context).size;
    return Dialog(
      child: SizedBox(
        width: size.width < 680 ? size.width - 48 : 620,
        height: size.height < 560 ? size.height - 48 : 480,
        child: Row(children: [
          SizedBox(width: 210, child: kinds),
          const VerticalDivider(width: 1),
          Expanded(
            child: f == null
                ? const Center(child: Text('Keine gefunden.'))
                : valueList(f, on, values),
          ),
        ]),
      ),
    );
  }

  /// The values of the chosen kind of filter, with a search box.
  Widget valueList(Map<String, dynamic> f, String on, List<Map> values) => Column(children: [
                    Padding(
                      padding: const EdgeInsets.all(8),
                      child: TextField(
                        controller: needle,
                        onChanged: (_) => setState(() {}),
                        decoration: InputDecoration(isDense: true, hintText: '${filterName(cur)} suchen …', prefixIcon: const Icon(Icons.search, size: 18), border: const OutlineInputBorder()),
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
                                  title: Text(valueName(cur, v)),
                                  trailing: Text('${values[i]['count'] ?? ''}', style: TextStyle(color: context.faint)),
                                  onTap: () => set(cur, v == on ? null : v),
                                );
                              },
                            ),
                    ),
                  ]);
}
