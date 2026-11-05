import 'package:flutter/material.dart';
import '../api.dart';
import '../widgets/poster.dart';
import 'home.dart';
import 'person.dart';
import 'package:url_launcher/url_launcher.dart';
import 'edit.dart';
import 'player.dart';
import 'playlists.dart';

const _gold = Color(0xFFE5A00D);

String fmtMs(int ms) {
  final m = ms ~/ 60000;
  return m >= 60 ? '${m ~/ 60} h ${m % 60} min' : '$m min';
}

String fmtClock(int ms) {
  final s = ms ~/ 1000;
  final h = s ~/ 3600, m = (s % 3600) ~/ 60, sec = s % 60;
  final mm = m.toString().padLeft(2, '0'), ss = sec.toString().padLeft(2, '0');
  return h > 0 ? '$h:$mm:$ss' : '$m:$ss';
}

String fmtSize(num b) => b > 1e9 ? '${(b / 1e9).toStringAsFixed(1)} GB' : '${(b / 1e6).toStringAsFixed(0)} MB';

const _containers = {'matroska': 'MKV', 'mov': 'MP4', 'mp4': 'MP4', 'mpegts': 'TS', 'avi': 'AVI', 'webm': 'WebM'};
String _container(Map m) => _containers['${m['container'] ?? ''}'.toLowerCase()] ?? '${m['container'] ?? ''}'.toUpperCase();

String _channels(dynamic n) => const {1: 'Mono', 2: 'Stereo', 3: '2.1', 6: '5.1', 7: '6.1', 8: '7.1'}[n] ?? (n == null ? '' : '$n Kanäle');

const _scoreNames = {
  'imdb': 'IMDb',
  'rotten_tomatoes': 'Rotten Tomatoes',
  'rotten_tomatoes_audience': 'RT Publikum',
  'metacritic': 'Metacritic',
  'tmdb': 'TMDB',
};

List<String> _strings(dynamic v) => v is List ? v.map((e) => '$e').where((e) => e.isNotEmpty).toList() : const [];

/// Movie, show, season or episode: the same details as the web app (scores, credits, cast, file info,
/// seasons / episodes, chapters, extras, related rows) and the same buttons (play, watched, favorite,
/// watchlist, your rating).
class DetailScreen extends StatefulWidget {
  final Api api;
  final String id;
  const DetailScreen({super.key, required this.api, required this.id});
  @override
  State<DetailScreen> createState() => _DetailScreenState();
}

class _DetailScreenState extends State<DetailScreen> {
  Map<String, dynamic>? it;
  Map<String, dynamic> topExtra = {}; // seasons and episodes borrow the show's cast, scores, ...
  List<Map<String, dynamic>> kids = [];
  List<Map<String, dynamic>> chapters = [];
  List<Map<String, dynamic>> related = [];
  String? error;
  bool admin = false;

  Api get api => widget.api;

  @override
  void initState() {
    super.initState();
    api.get('/api/me').then((m) {
      if (mounted && m is Map) setState(() => admin = m['admin'] == true);
    }).catchError((_) {});
    load();
  }

  Future<void> load() async {
    try {
      final item = await api.item(widget.id);
      final type = item['type'];
      final top = item['grandparentRatingKey'] ?? (type == 'season' ? item['parentRatingKey'] : null);
      final results = await Future.wait<dynamic>([
        top != null ? api.item('$top').then<Map<String, dynamic>>((v) => v, onError: (_) => <String, dynamic>{}) : Future.value(<String, dynamic>{}),
        (type == 'show' || type == 'season') ? api.children(widget.id) : Future.value(<Map<String, dynamic>>[]),
        (item['Media'] as List?)?.isNotEmpty ?? false ? api.chapters(widget.id) : Future.value(<Map<String, dynamic>>[]),
        (type == 'movie' || type == 'show') && item['extra']?['metadata_source'] != null ? api.related(widget.id) : Future.value(<Map<String, dynamic>>[]),
      ]);
      if (!mounted) return;
      setState(() {
        it = item;
        topExtra = Map<String, dynamic>.from((results[0] as Map<String, dynamic>)['extra'] ?? const {});
        kids = results[1] as List<Map<String, dynamic>>;
        chapters = results[2] as List<Map<String, dynamic>>;
        related = results[3] as List<Map<String, dynamic>>;
      });
    } catch (e) {
      if (mounted) setState(() => error = '$e');
    }
  }

  String get key => '${it!['ratingKey'] ?? it!['id'] ?? widget.id}';

  Future<void> play(Map<String, dynamic> item) async {
    await Navigator.push(context, MaterialPageRoute(builder: (_) => PlayerScreen(api: api, item: item)));
    if (mounted) load(); // refresh resume position and watched marks
  }

  Future<void> playNextEpisode() async {
    final leaves = await api.allLeaves(widget.id);
    if (leaves.isEmpty) return;
    final e = leaves.firstWhere((x) => ((x['viewCount'] ?? 0) as int) == 0, orElse: () => leaves.first);
    play(e);
  }

  Future<void> act(Future<void> Function() f) async {
    try {
      await f();
      await load();
    } catch (e) {
      if (mounted) ScaffoldMessenger.of(context).showSnackBar(SnackBar(content: Text('$e')));
    }
  }

  @override
  Widget build(BuildContext context) {
    if (error != null) return Scaffold(appBar: AppBar(), body: Center(child: Text(error!)));
    final item = it;
    if (item == null) return Scaffold(appBar: AppBar(), body: const Center(child: CircularProgressIndicator()));
    final type = '${item['type']}';
    final x = Map<String, dynamic>.from(item['extra'] ?? const {});
    final media = (item['Media'] as List?)?.isNotEmpty ?? false ? Map<String, dynamic>.from((item['Media'] as List).first) : null;
    final playable = (type == 'movie' || type == 'episode') && item['missing'] != true && media != null;
    final off = (item['viewOffset'] ?? 0) as int;
    final leafCount = (item['leafCount'] ?? 0) as int;
    final watched = leafCount > 0 ? item['viewedLeafCount'] == leafCount : ((item['viewCount'] ?? 0) as int) > 0;
    final ratings = Map<String, dynamic>.from(x['ratings'] ?? (type == 'season' ? topExtra['ratings'] : null) ?? const {});
    final cast = ((x['cast'] ?? (type != 'movie' ? topExtra['cast'] : null)) as List? ?? const []).map((e) => Map<String, dynamic>.from(e)).toList();
    final crew = <List<String>>[
      ['Regie', ..._strings(x['directors'])],
      ['Drehbuch', ..._strings(x['writers'])],
      ['Idee', ..._strings(x['creators'] ?? (type != 'show' ? topExtra['creators'] : null))],
    ].where((r) => r.length > 1).toList();
    final genres = _strings(item['genres']);
    final ids = Map<String, dynamic>.from(x['ids'] ?? topExtra['ids'] ?? const {});
    final links = <List<String>>[
      if (ids['imdb'] != null) ['IMDb', 'https://www.imdb.com/title/${ids['imdb']}'],
      if (ids['tmdb'] != null) ['TMDB', 'https://www.themoviedb.org/${type == 'movie' ? 'movie' : 'tv'}/${ids['tmdb']}'],
      if (type == 'movie' && (ids['tmdb'] != null || ids['imdb'] != null)) ['Letterboxd', ids['tmdb'] != null ? 'https://letterboxd.com/tmdb/${ids['tmdb']}' : 'https://letterboxd.com/imdb/${ids['imdb']}'],
      if (ids['tvmaze'] != null) ['TVmaze', 'https://www.tvmaze.com/shows/${ids['tvmaze']}'],
      if (ids['wikipedia'] != null) ['Wikipedia', '${ids['wikipedia']}'],
      if (ids['wikidata'] != null) ['Wikidata', 'https://www.wikidata.org/wiki/${ids['wikidata']}'],
    ];
    final online = (type == 'movie' || type == 'show' ? (x['trailers'] as List? ?? const []) : const [])
        .where((v) => v is Map && v['key'] != null)
        .map((v) => Map<String, dynamic>.from(v as Map))
        .toList();
    final released = '${x['originally_available'] ?? ''}';
    final extras = (item['Extras'] as List? ?? const []).map((e) => Map<String, dynamic>.from(e)).toList();
    final trailer = extras.where((e) => e['extraType'] == 'trailer').firstOrNull;
    final dur = (item['duration'] ?? 0) as int;
    final meta = [
      if (item['year'] != null) '${item['year']}',
      if ((item['editionTitle'] ?? '').toString().isNotEmpty) '${item['editionTitle']}',
      if (type == 'episode') 'Staffel ${item['parentIndex']}',
      if (dur > 0) fmtMs(dur),
      if (item['contentRating'] != null) '${item['contentRating']}',
      if (item['rating'] != null && ratings.isEmpty) '★ ${item['rating']}',
      if (leafCount > 0) '${item['viewedLeafCount'] ?? 0}/$leafCount gesehen',
    ].join('  ·  ');
    final dim = const TextStyle(color: Colors.white60);

    final title = type == 'episode' ? '${item['index']}. ${item['title']}' : '${item['title']}';
    final crumbs = [
      if (item['grandparentTitle'] != null) '${item['grandparentTitle']}',
      if (item['parentTitle'] != null) '${item['parentTitle']}',
    ].join(' › ');

    return Scaffold(
      appBar: AppBar(actions: [if (admin) _adminMenu(item)]),
      body: ListView(padding: const EdgeInsets.all(20), children: [
        if (crumbs.isNotEmpty) Padding(padding: const EdgeInsets.only(bottom: 10), child: Text(crumbs, style: dim)),
        Row(crossAxisAlignment: CrossAxisAlignment.start, children: [
          SizedBox(
            width: 180,
            child: AspectRatio(
              aspectRatio: 2 / 3,
              child: ClipRRect(
                borderRadius: BorderRadius.circular(8),
                child: Image.network(api.img(item['thumb']), fit: BoxFit.cover, errorBuilder: (_, __, ___) => Container(color: Colors.white10)),
              ),
            ),
          ),
          const SizedBox(width: 20),
          Expanded(
            child: Column(crossAxisAlignment: CrossAxisAlignment.start, children: [
              Text(title, style: Theme.of(context).textTheme.headlineMedium),
              if (item['missing'] == true) const Text('Nicht verfügbar: die Datei fehlt auf der Festplatte.', style: TextStyle(color: Colors.redAccent)),
              const SizedBox(height: 6),
              Text(meta, style: dim),
              if (ratings.isNotEmpty) Padding(padding: const EdgeInsets.only(top: 8), child: _scores(ratings)),
              if ('${item['tagline'] ?? ''}'.isNotEmpty) Padding(padding: const EdgeInsets.only(top: 10), child: Text('${item['tagline']}', style: const TextStyle(fontStyle: FontStyle.italic))),
              if ('${item['summary'] ?? ''}'.isNotEmpty) Padding(padding: const EdgeInsets.only(top: 10), child: Text('${item['summary']}')),
              if (genres.isNotEmpty) Padding(padding: const EdgeInsets.only(top: 10), child: Wrap(spacing: 6, runSpacing: 6, children: [for (final g in genres) _tag(g)])),
              if (crew.isNotEmpty || item['studio'] != null || released.isNotEmpty || (x['original_title'] != null && x['original_title'] != item['title']))
                Padding(
                  padding: const EdgeInsets.only(top: 12),
                  child: _kv([
                    for (final r in crew) [r.first, r.skip(1).join(', ')],
                    if (item['studio'] != null) [type == 'show' ? 'Sender' : 'Studio', '${item['studio']}'],
                    if (x['original_title'] != null && x['original_title'] != item['title']) ['Originaltitel', '${x['original_title']}'],
                    if (released.isNotEmpty) [type == 'movie' ? 'Erschienen' : 'Erstausstrahlung', released.split('T').first],
                  ]),
                ),
              const SizedBox(height: 16),
              Wrap(spacing: 10, runSpacing: 10, crossAxisAlignment: WrapCrossAlignment.center, children: [
                if (playable && off > 0) FilledButton.icon(onPressed: () => play(item), icon: const Icon(Icons.play_arrow), label: Text('Fortsetzen bei ${fmtClock(off)}')),
                if (playable && off > 0) OutlinedButton(onPressed: () => play({...item, 'viewOffset': 0}), child: const Text('Von vorn')),
                if (playable && off == 0) FilledButton.icon(onPressed: () => play(item), icon: const Icon(Icons.play_arrow), label: const Text('Abspielen')),
                if (trailer != null) OutlinedButton.icon(onPressed: () => play(trailer), icon: const Icon(Icons.movie_outlined), label: const Text('Trailer'))
                else if (online.isNotEmpty) OutlinedButton.icon(onPressed: () => _open('https://www.youtube.com/watch?v=${online.first['key']}'), icon: const Icon(Icons.movie_outlined), label: const Text('Trailer (online)')),
                if (!playable && leafCount > 0 && (type == 'show' || type == 'season'))
                  FilledButton.icon(onPressed: playNextEpisode, icon: const Icon(Icons.play_arrow), label: const Text('Nächste Folge')),
                _toggle(watched ? Icons.check_circle : Icons.check_circle_outline, watched, watched ? 'Als ungesehen markieren' : 'Als gesehen markieren', () => act(() => api.setWatched(key, !watched))),
                _toggle(item['favorite'] == true ? Icons.favorite : Icons.favorite_border, item['favorite'] == true, item['favorite'] == true ? 'Aus Favoriten entfernen' : 'Zu Favoriten', () => act(() => api.setFavorite(key, item['favorite'] != true))),
                if (type != 'clip') _toggle(item['watchlist'] == true ? Icons.bookmark : Icons.bookmark_border, item['watchlist'] == true, item['watchlist'] == true ? 'Von der Merkliste entfernen' : 'Auf die Merkliste', () => act(() => api.setWatchlist(key, item['watchlist'] != true))),
                if (playable || type == 'episode') OutlinedButton.icon(onPressed: () => addToPlaylistDialog(context, api, key), icon: const Icon(Icons.playlist_add), label: const Text('Zur Playlist')),
                _stars(item),
              ]),
              if (links.isNotEmpty)
                Padding(
                  padding: const EdgeInsets.only(top: 12),
                  child: Wrap(spacing: 8, runSpacing: 4, children: [for (final l in links) ActionChip(avatar: const Icon(Icons.open_in_new, size: 14), label: Text(l[0]), onPressed: () => _open(l[1]))]),
                ),
              if (media != null) Padding(padding: const EdgeInsets.only(top: 16), child: _fileInfo(item, media)),
            ]),
          ),
        ]),
        if (cast.isNotEmpty) ...[_h('Besetzung'), SizedBox(height: 150, child: ListView(scrollDirection: Axis.horizontal, children: [for (final p in cast) _person(p)]))],
        if (kids.isNotEmpty) ...[
          _h(type == 'show' ? 'Staffeln' : 'Folgen'),
          if (type == 'show')
            Wrap(spacing: 12, runSpacing: 16, children: [for (final k in kids) Poster(api: api, item: k, onTap: () => openItem(context, api, k))])
          else
            for (var i = 0; i < kids.length; i++) _episode(kids[i]),
        ],
        if (chapters.isNotEmpty) ...[
          _h('Kapitel'),
          SizedBox(
            height: 170,
            child: ListView(scrollDirection: Axis.horizontal, children: [
              for (final c in chapters)
                InkWell(
                  onTap: () => play({...item, 'viewOffset': (c['start'] ?? 0) as int}),
                  child: Container(
                    width: 170,
                    margin: const EdgeInsets.only(right: 12),
                    child: Column(crossAxisAlignment: CrossAxisAlignment.start, children: [
                      AspectRatio(
                        aspectRatio: 16 / 9,
                        child: ClipRRect(borderRadius: BorderRadius.circular(6), child: Image.network(api.img(c['thumb']), fit: BoxFit.cover, errorBuilder: (_, __, ___) => Container(color: Colors.white10))),
                      ),
                      Text('${c['title']}'.replaceFirstMapped(RegExp(r'^Chapter (\d+)$'), (m) => 'Kapitel ${m[1]}'), maxLines: 1, overflow: TextOverflow.ellipsis),
                      Text(fmtClock((c['start'] ?? 0) as int), style: const TextStyle(fontSize: 12, color: Colors.white54)),
                    ]),
                  ),
                ),
            ]),
          ),
        ],
        if (extras.isNotEmpty || online.isNotEmpty) ...[
          _h('Extras'),
          SizedBox(
            height: 170,
            child: ListView(scrollDirection: Axis.horizontal, children: [
              for (final e in extras)
                InkWell(
                  onTap: () => play(e),
                  child: Container(
                    width: 170,
                    margin: const EdgeInsets.only(right: 12),
                    child: Column(crossAxisAlignment: CrossAxisAlignment.start, children: [
                      AspectRatio(
                        aspectRatio: 16 / 9,
                        child: ClipRRect(borderRadius: BorderRadius.circular(6), child: Image.network(api.img(e['thumb']), fit: BoxFit.cover, errorBuilder: (_, __, ___) => Container(color: Colors.white10, child: const Icon(Icons.play_circle_outline)))),
                      ),
                      Text('${e['title']}', maxLines: 1, overflow: TextOverflow.ellipsis),
                      Text(fmtMs((e['duration'] ?? 0) as int), style: const TextStyle(fontSize: 12, color: Colors.white54)),
                    ]),
                  ),
                ),
              for (final v in online)
                InkWell(
                  onTap: () => _open('https://www.youtube.com/watch?v=${v['key']}'),
                  child: Container(
                    width: 170,
                    margin: const EdgeInsets.only(right: 12),
                    child: Column(crossAxisAlignment: CrossAxisAlignment.start, children: [
                      AspectRatio(
                        aspectRatio: 16 / 9,
                        child: ClipRRect(borderRadius: BorderRadius.circular(6), child: Image.network('https://i.ytimg.com/vi/${v['key']}/mqdefault.jpg', fit: BoxFit.cover, errorBuilder: (_, __, ___) => Container(color: Colors.white10, child: const Icon(Icons.play_circle_outline)))),
                      ),
                      Text('${v['name'] ?? 'Trailer'}', maxLines: 1, overflow: TextOverflow.ellipsis),
                      const Text('online · YouTube', style: TextStyle(fontSize: 12, color: Colors.white54)),
                    ]),
                  ),
                ),
            ]),
          ),
        ],
        for (final hub in related)
          if ((hub['Metadata'] as List?)?.isNotEmpty ?? false) ...[
            _h('${hub['title']}'),
            SizedBox(
              height: 280,
              child: ListView.separated(
                scrollDirection: Axis.horizontal,
                itemCount: (hub['Metadata'] as List).length,
                separatorBuilder: (_, __) => const SizedBox(width: 12),
                itemBuilder: (_, i) {
                  final m = Map<String, dynamic>.from((hub['Metadata'] as List)[i]);
                  return Poster(api: api, item: m, onTap: () => openItem(context, api, m));
                },
              ),
            ),
          ],
      ]),
    );
  }

  Widget _h(String t) => Padding(padding: const EdgeInsets.only(top: 24, bottom: 8), child: Text(t, style: Theme.of(context).textTheme.titleMedium));

  Widget _adminMenu(Map<String, dynamic> item) {
    final type = '${item['type']}';
    final matchable = type == 'movie' || type == 'show';
    final matched = item['extra']?['metadata_source'] != null;
    void say(String m) => ScaffoldMessenger.of(context).showSnackBar(SnackBar(content: Text(m)));
    return PopupMenuButton<String>(
      tooltip: 'Verwalten',
      onSelected: (v) async {
        try {
          switch (v) {
            case 'edit':
              if (await editDetailsDialog(context, api, item)) load();
            case 'match':
              if (await matchDialog(context, api, item)) load();
            case 'unmatch':
              final ok = await showDialog<bool>(
                context: context,
                builder: (d) => AlertDialog(
                  content: Text('Die Online-Details von „${item['title']}“ entfernen? Der Titel fällt auf den Dateinamen zurück und wird erst wieder automatisch zugeordnet, wenn du eine Zuordnung wählst.'),
                  actions: [TextButton(onPressed: () => Navigator.pop(d, false), child: const Text('Abbrechen')), FilledButton(onPressed: () => Navigator.pop(d, true), child: const Text('Zuordnung aufheben'))],
                ),
              );
              if (ok == true) {
                final r = await api.delete('/library/metadata/$key/match');
                if (r is Map && r['message'] != null) say('${r['message']}');
                load();
              }
            case 'refresh':
              say('Lade Details…');
              final r = await api.post('/library/metadata/$key/refresh');
              if (r is Map && r['message'] != null) say('${r['message']}');
              load();
            case 'analyze':
              await api.post('/library/metadata/$key/analyze');
              say('Analysiere…');
          }
        } catch (e) {
          say('$e');
        }
      },
      itemBuilder: (_) => [
        if (matchable) const PopupMenuItem(value: 'refresh', child: Text('Metadaten aktualisieren')),
        if (item['Media'] != null) const PopupMenuItem(value: 'analyze', child: Text('Analysieren')),
        const PopupMenuItem(value: 'edit', child: Text('Details bearbeiten')),
        if (matchable) PopupMenuItem(value: 'match', child: Text(matched ? 'Zuordnung korrigieren…' : 'Zuordnen…')),
        if (matchable && matched) const PopupMenuItem(value: 'unmatch', child: Text('Zuordnung aufheben')),
      ],
    );
  }

  Future<void> _open(String url) async {
    if (!await launchUrl(Uri.parse(url), mode: LaunchMode.externalApplication) && mounted) {
      ScaffoldMessenger.of(context).showSnackBar(const SnackBar(content: Text('Der Link ließ sich nicht öffnen')));
    }
  }

  Widget _tag(String t) => Container(
        padding: const EdgeInsets.symmetric(horizontal: 10, vertical: 4),
        decoration: BoxDecoration(color: Colors.white10, borderRadius: BorderRadius.circular(12)),
        child: Text(t, style: const TextStyle(fontSize: 12)),
      );

  Widget _kv(List<List<String>> rows) => Table(
        columnWidths: const {0: IntrinsicColumnWidth()},
        defaultVerticalAlignment: TableCellVerticalAlignment.top,
        children: [
          for (final r in rows)
            TableRow(children: [
              Padding(padding: const EdgeInsets.only(right: 16, bottom: 3), child: Text(r[0], style: const TextStyle(color: Colors.white54))),
              Padding(padding: const EdgeInsets.only(bottom: 3), child: Text(r[1])),
            ]),
        ],
      );

  Widget _scores(Map<String, dynamic> r) {
    final chips = <Widget>[];
    for (final e in _scoreNames.entries) {
      final s = r[e.key];
      if (s is! Map || s['score'] == null || '${s['score']}'.isEmpty) continue;
      final count = s['count'] ?? s['votes'];
      chips.add(Tooltip(
        message: count != null ? '${e.value}: ${s['score']} · $count ${s['count_label'] == 'reviews' ? 'Kritiken' : 'Stimmen'}' : '${e.value}: ${s['score']}',
        child: Container(
          padding: const EdgeInsets.symmetric(horizontal: 10, vertical: 5),
          decoration: BoxDecoration(color: Colors.white10, borderRadius: BorderRadius.circular(6)),
          child: Row(mainAxisSize: MainAxisSize.min, children: [
            Text(e.value, style: const TextStyle(fontSize: 12, color: Colors.white60)),
            const SizedBox(width: 6),
            Text('${s['score']}', style: const TextStyle(fontWeight: FontWeight.bold)),
          ]),
        ),
      ));
    }
    return Wrap(spacing: 8, runSpacing: 8, children: chips);
  }

  Widget _toggle(IconData icon, bool on, String tip, VoidCallback onTap) =>
      IconButton(tooltip: tip, onPressed: onTap, icon: Icon(icon, color: on ? _gold : null), style: IconButton.styleFrom(backgroundColor: Colors.white10));

  /// Five stars on the server's 0-10 scale (each star is worth 2); tap the active last star's "clear" to remove.
  Widget _stars(Map<String, dynamic> item) {
    final cur = ((item['userRating'] ?? 0) as num).round();
    return Row(mainAxisSize: MainAxisSize.min, children: [
      for (var i = 1; i <= 5; i++)
        InkResponse(
          onTap: () => act(() => api.rate(key, i * 2)),
          child: Icon(cur >= i * 2 ? Icons.star : (cur == i * 2 - 1 ? Icons.star_half : Icons.star_border), color: cur >= i * 2 - 1 ? _gold : Colors.white38, size: 26),
        ),
      if (cur > 0) TextButton(onPressed: () => act(() => api.rate(key, -1)), child: const Text('löschen')),
    ]);
  }

  Widget _fileInfo(Map<String, dynamic> item, Map media) {
    final type = item['type'];
    final parts = (media['Part'] as List?) ?? const [];
    final subs = ((media['subtitles'] as List?) ?? const []).map((s) => '${(s is Map ? (s['language'] ?? s['file'] ?? s['codec']) : s) ?? ''}').where((s) => s.isNotEmpty).toSet().join(', ');
    return Container(
      padding: const EdgeInsets.all(12),
      decoration: BoxDecoration(color: Colors.white10, borderRadius: BorderRadius.circular(8)),
      child: _kv([
        ['Dateityp', '${_container(media)}${media['size'] != null ? ' · ${fmtSize(media['size'])}' : ''}'],
        if (type != 'track')
          [
            'Video',
            media['videoCodec'] == null
                ? 'unbekannt'
                : ['${media['videoCodec']}'.toUpperCase(), '${media['width']}×${media['height']}', if ((media['bitDepth'] ?? 0) > 8) '${media['bitDepth']}-bit'].join(' · ')
          ],
        ['Audio', media['audioCodec'] == null ? '–' : ['${media['audioCodec']}'.toUpperCase(), _channels(media['audioChannels'])].where((s) => s.isNotEmpty).join(' · ')],
        ['Bitrate', media['bitrate'] != null ? '${((media['bitrate'] as num) / 1000).toStringAsFixed(1)} Mbit/s' : '–'],
        if (type != 'track') ['Untertitel', subs.isEmpty ? 'keine' : subs],
        if (media['file'] != null) ['Pfad', '${media['file']}'],
        if (parts.length > 1) ['Teile', '${parts.length} Dateien, nacheinander abgespielt'],
      ]),
    );
  }

  Widget _person(Map<String, dynamic> p) {
    final name = '${p['name'] ?? '?'}';
    final src = p['thumb'] != null ? api.img('${p['thumb']}') : '${p['photo'] ?? ''}';
    final initials = name.split(' ').where((w) => w.isNotEmpty).map((w) => w[0]).take(2).join();
    return InkWell(
      borderRadius: BorderRadius.circular(8),
      onTap: () => Navigator.push(context, MaterialPageRoute(builder: (_) => PersonScreen(api: api, keyId: '${p['key'] ?? 'byname'}', name: name))),
      child: Container(
      width: 100,
      margin: const EdgeInsets.only(right: 12),
      child: Column(children: [
        CircleAvatar(
          radius: 38,
          backgroundColor: Colors.white10,
          foregroundImage: src.isEmpty ? null : NetworkImage(src),
          onForegroundImageError: src.isEmpty ? null : (_, __) {},
          child: Text(initials),
        ),
        const SizedBox(height: 6),
        Text(name, maxLines: 2, textAlign: TextAlign.center, overflow: TextOverflow.ellipsis, style: const TextStyle(fontSize: 12, fontWeight: FontWeight.w500)),
        if (p['role'] != null) Text('${p['role']}', maxLines: 1, textAlign: TextAlign.center, overflow: TextOverflow.ellipsis, style: const TextStyle(fontSize: 11, color: Colors.white54)),
      ]),
      ),
    );
  }

  Widget _episode(Map<String, dynamic> e) {
    final seen = ((e['viewCount'] ?? 0) as int) > 0;
    final off = (e['viewOffset'] ?? 0) as int;
    final sub = [
      if (((e['duration'] ?? 0) as int) > 0) fmtMs(e['duration']),
      if (seen) '✓ gesehen',
      if (off > 0) 'gestoppt bei ${fmtClock(off)}',
    ].join(' · ');
    return InkWell(
      onTap: () => openItem(context, api, e),
      child: Padding(
        padding: const EdgeInsets.symmetric(vertical: 6),
        child: Row(crossAxisAlignment: CrossAxisAlignment.start, children: [
          SizedBox(
            width: 140,
            child: AspectRatio(
              aspectRatio: 16 / 9,
              child: ClipRRect(borderRadius: BorderRadius.circular(6), child: Image.network(api.img(e['thumb']), fit: BoxFit.cover, errorBuilder: (_, __, ___) => Container(color: Colors.white10))),
            ),
          ),
          const SizedBox(width: 12),
          Expanded(
            child: Column(crossAxisAlignment: CrossAxisAlignment.start, children: [
              Text(e['index'] != null ? '${e['index']}. ${e['title']}' : '${e['title']}', style: const TextStyle(fontWeight: FontWeight.w600)),
              Text(sub, style: const TextStyle(fontSize: 12, color: Colors.white54)),
              if ('${e['summary'] ?? ''}'.isNotEmpty) Padding(padding: const EdgeInsets.only(top: 4), child: Text('${e['summary']}', maxLines: 3, overflow: TextOverflow.ellipsis, style: const TextStyle(fontSize: 13, color: Colors.white70))),
            ]),
          ),
          IconButton(tooltip: 'Abspielen', onPressed: () => play(e), icon: const Icon(Icons.play_circle_fill, color: _gold)),
        ]),
      ),
    );
  }
}
