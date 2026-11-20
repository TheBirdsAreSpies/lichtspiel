import 'dart:async';
import 'package:flutter/material.dart';
import '../api.dart';
import '../theme.dart';

const _fieldNames = {
  'title': 'Titel', 'kind': 'Art (movie, show, episode …)', 'year': 'Jahr', 'genre': 'Genre', 'rating': 'Bewertung', 'user_rating': 'Meine Bewertung',
  'content_rating': 'Altersfreigabe', 'studio': 'Studio / Sender', 'summary': 'Handlung', 'show': 'Serie', 'artist': 'Künstler',
  'director': 'Regie', 'actor': 'Darsteller', 'writer': 'Drehbuch', 'country': 'Land', 'collection': 'Sammlung', 'label': 'Label',
  'edition': 'Edition', 'folder': 'Ordner', 'unwatched': 'Ungesehen', 'in_progress': 'Angefangen', 'added_at': 'Hinzugefügt', 'last_viewed_at': 'Zuletzt gesehen',
  'view_count': 'Anzahl Wiedergaben', 'duration': 'Länge', 'release_date': 'Erscheinungsdatum', 'video_resolution': 'Auflösung', 'video_codec': 'Videocodec',
  'audio_codec': 'Audiocodec', 'audio_language': 'Tonsprache', 'subtitle_language': 'Untertitelsprache', 'hdr': 'HDR', 'dovi': 'Dolby Vision',
  'duplicate': 'Doppelt', 'unmatched': 'Nicht zugeordnet', 'favorite': 'Favorit', 'watchlist': 'Auf der Merkliste',
};
const _opNames = {
  'is': 'ist', 'is_not': 'ist nicht', 'contains': 'enthält', 'not_contains': 'enthält nicht', 'starts_with': 'beginnt mit', 'ends_with': 'endet mit',
  'in': 'ist eines von (Komma)', 'gt': 'größer als', 'gte': 'mindestens', 'lt': 'kleiner als', 'lte': 'höchstens', 'between': 'zwischen (niedrig,hoch)',
  'in_last_days': 'in den letzten … Tagen', 'not_in_last_days': 'nicht in den letzten … Tagen',
};
const _opsByType = {
  'bool': ['is', 'is_not'],
  'date': ['in_last_days', 'not_in_last_days', 'gt', 'lt'],
  'number': ['is', 'is_not', 'gt', 'gte', 'lt', 'lte', 'between', 'in'],
  'text': ['is', 'is_not', 'contains', 'not_contains', 'starts_with', 'ends_with', 'in'],
};
const _sorts = {
  'title': 'Titel', 'year:desc': 'Jahr (neueste)', 'year': 'Jahr (älteste)', 'added_at:desc': 'Zuletzt hinzugefügt', 'rating:desc': 'Bewertung',
  'duration:desc': 'Länge', 'last_viewed_at:desc': 'Zuletzt gesehen', 'view_count:desc': 'Meist gesehen', 'random': 'Zufällig', 'episode_order': 'Serie, Staffel, Folge',
};

/// Creates a smart playlist or edits the rules of one. Returns the playlist id, or null if cancelled.
Future<String?> smartEditor(BuildContext context, Api api, {Map<String, dynamic>? playlist}) {
  return showDialog<String>(context: context, builder: (_) => _SmartDialog(api: api, playlist: playlist));
}

class _SmartDialog extends StatefulWidget {
  final Api api;
  final Map<String, dynamic>? playlist;
  const _SmartDialog({required this.api, this.playlist});
  @override
  State<_SmartDialog> createState() => _SmartDialogState();
}

class _SmartDialogState extends State<_SmartDialog> {
  List<Map<String, dynamic>> fields = [], sections = [];
  late Map<String, dynamic> q;
  late final TextEditingController name;
  late final TextEditingController limit;
  bool shared = false, pinned = false, loaded = false, saving = false;
  String preview = '';
  String? error;
  Timer? timer;

  Api get api => widget.api;
  Map<String, dynamic>? get pl => widget.playlist;

  @override
  void initState() {
    super.initState();
    final src = pl?['query'];
    q = src is Map
        ? Map<String, dynamic>.from(src)
        : {'match_all': true, 'rules': [{'field': 'kind', 'op': 'is', 'value': 'movie'}], 'sort': 'title', 'limit': null, 'sections': []};
    q['rules'] = [for (final r in (q['rules'] as List? ?? const [])) Map<String, dynamic>.from(r as Map)];
    q['sections'] = [...(q['sections'] as List? ?? const [])];
    name = TextEditingController(text: pl == null ? '' : '${pl!['title']}');
    limit = TextEditingController(text: q['limit'] == null ? '' : '${q['limit']}');
    shared = pl?['shared'] == true;
    pinned = pl?['pinned'] == true;
    load();
  }

  @override
  void dispose() {
    timer?.cancel();
    name.dispose();
    limit.dispose();
    super.dispose();
  }

  Future<void> load() async {
    try {
      final f = await api.get('/api/filters');
      fields = [for (final x in (f?['fields'] as List? ?? const [])) if (x['type'] != 'sort') Map<String, dynamic>.from(x)];
      sections = await api.sections();
      if (!mounted) return;
      setState(() => loaded = true);
      upd();
    } catch (e) {
      if (mounted) setState(() => error = '$e');
    }
  }

  Map<String, dynamic> fieldOf(String f) => fields.firstWhere((x) => x['field'] == f, orElse: () => fields.first);
  List<Map<String, dynamic>> get rules => (q['rules'] as List).cast<Map<String, dynamic>>();

  void upd() {
    timer?.cancel();
    timer = Timer(const Duration(milliseconds: 350), () async {
      try {
        final r = await api.post('/playlists/preview', {'query': q});
        final mc = r?['MediaContainer'];
        final list = ((mc?['Metadata'] as List?) ?? const []).take(6).map((m) {
          if (m['type'] == 'episode') return '${m['grandparentTitle']} S${m['parentIndex']}E${m['index']}';
          if (m['type'] == 'track') return '${m['grandparentTitle']} · ${m['title']}';
          return '${m['title']}';
        }).join(', ');
        final total = mc?['totalSize'] ?? 0;
        if (mounted) setState(() => preview = '$total Titel passen: $list${total > 6 ? ' …' : ''}');
      } catch (e) {
        if (mounted) setState(() => preview = '$e');
      }
    });
  }

  dynamic parse(Map<String, dynamic> r, String v) {
    final op = '${r['op']}';
    final type = fieldOf('${r['field']}')['type'];
    if (op == 'between') return v.split(',').map((e) => num.tryParse(e.trim()) ?? 0).toList();
    if (op == 'in') return v.split(',').map((e) => e.trim()).where((e) => e.isNotEmpty).toList();
    if ((type == 'number' || op.contains('days')) && v.trim().isNotEmpty && num.tryParse(v.trim()) != null) return num.parse(v.trim());
    return v;
  }

  String show(dynamic v) => v is List ? v.join(',') : '${v ?? ''}';

  Widget ruleRow(int i) {
    final r = rules[i];
    final f = fieldOf('${r['field']}');
    final type = '${f['type']}';
    final ops = _opsByType[type] ?? _opsByType['text']!;
    final op = ops.contains(r['op']) ? '${r['op']}' : ops.first;
    return Padding(
      padding: const EdgeInsets.symmetric(vertical: 4),
      child: Row(crossAxisAlignment: CrossAxisAlignment.start, children: [
        Expanded(
          flex: 3,
          child: DropdownButtonFormField<String>(
            initialValue: '${f['field']}',
            isExpanded: true,
            items: [for (final x in fields) DropdownMenuItem(value: '${x['field']}', child: Text(_fieldNames['${x['field']}'] ?? '${x['field']}', overflow: TextOverflow.ellipsis))],
            onChanged: (v) => setState(() {
              r['field'] = v;
              final nf = fieldOf('$v');
              r['op'] = (_opsByType[nf['type']] ?? _opsByType['text']!).first;
              r['value'] = nf['type'] == 'bool' ? true : '';
              upd();
            }),
          ),
        ),
        const SizedBox(width: 8),
        Expanded(
          flex: 3,
          child: DropdownButtonFormField<String>(
            initialValue: op,
            isExpanded: true,
            items: [for (final o in ops) DropdownMenuItem(value: o, child: Text(_opNames[o] ?? o, overflow: TextOverflow.ellipsis))],
            onChanged: (v) => setState(() {
              r['op'] = v;
              upd();
            }),
          ),
        ),
        const SizedBox(width: 8),
        Expanded(
          flex: 3,
          child: type == 'bool'
              ? DropdownButtonFormField<bool>(
                  initialValue: r['value'] == false ? false : true,
                  items: const [DropdownMenuItem(value: true, child: Text('ja')), DropdownMenuItem(value: false, child: Text('nein'))],
                  onChanged: (v) => setState(() {
                    r['value'] = v ?? true;
                    upd();
                  }),
                )
              : TextFormField(
                  key: ValueKey('v$i-${r['field']}-$op'),
                  initialValue: show(r['value']),
                  decoration: InputDecoration(hintText: op == 'between' ? 'niedrig,hoch' : op.contains('days') ? 'Tage' : 'Wert'),
                  onChanged: (v) {
                    r['value'] = parse(r, v);
                    upd();
                  },
                ),
        ),
        IconButton(
          tooltip: 'Regel entfernen',
          icon: const Icon(Icons.close),
          onPressed: () => setState(() {
            rules.removeAt(i);
            upd();
          }),
        ),
      ]),
    );
  }

  Future<void> save() async {
    if (name.text.trim().isEmpty) {
      setState(() => error = 'Gib der Playlist einen Namen');
      return;
    }
    q['limit'] = int.tryParse(limit.text.trim());
    q['rules'] = rules.where((r) => r['value'] != '' && r['value'] != null).toList();
    setState(() {
      saving = true;
      error = null;
    });
    try {
      String id;
      if (pl != null) {
        await api.put('/playlists/${pl!['id']}', {'title': name.text.trim(), 'query': q, 'shared': shared, 'pinned': pinned});
        id = '${pl!['id']}';
      } else {
        final r = await api.post('/playlists', {'title': name.text.trim(), 'kind': 'smart', 'query': q, 'shared': shared, 'pinned': pinned});
        final m = (r?['MediaContainer']?['Metadata'] as List?)?.firstOrNull;
        id = '${m?['id'] ?? m?['ratingKey'] ?? r?['id']}';
      }
      if (mounted) Navigator.pop(context, id);
    } catch (e) {
      if (mounted) {
        setState(() {
          saving = false;
          error = '$e';
        });
      }
    }
  }

  @override
  Widget build(BuildContext context) {
    final isDefault = pl?['default'] == true;
    return AlertDialog(
      title: Text(pl == null ? 'Neue intelligente Playlist' : 'Regeln bearbeiten'),
      content: SizedBox(
        width: 720,
        child: !loaded
            ? SizedBox(height: 120, child: Center(child: error != null ? Text(error!) : const CircularProgressIndicator()))
            : SingleChildScrollView(
                child: Column(mainAxisSize: MainAxisSize.min, crossAxisAlignment: CrossAxisAlignment.start, children: [
                  TextField(controller: name, decoration: const InputDecoration(labelText: 'Name', hintText: 'z. B. Ungesehene Sci-Fi aus den 90ern')),
                  const SizedBox(height: 12),
                  DropdownButtonFormField<bool>(
                    initialValue: q['match_all'] != false,
                    decoration: const InputDecoration(labelText: 'Titel aufnehmen, die passen auf'),
                    items: const [DropdownMenuItem(value: true, child: Text('alle diese Regeln')), DropdownMenuItem(value: false, child: Text('eine dieser Regeln'))],
                    onChanged: (v) => setState(() {
                      q['match_all'] = v ?? true;
                      upd();
                    }),
                  ),
                  const SizedBox(height: 8),
                  for (var i = 0; i < rules.length; i++) ruleRow(i),
                  Align(
                    alignment: Alignment.centerLeft,
                    child: TextButton.icon(
                      icon: const Icon(Icons.add),
                      label: const Text('Regel hinzufügen'),
                      onPressed: () => setState(() => rules.add({'field': 'genre', 'op': 'is', 'value': ''})),
                    ),
                  ),
                  Row(children: [
                    Expanded(
                      child: DropdownButtonFormField<String>(
                        initialValue: _sorts.containsKey(q['sort']) ? '${q['sort']}' : 'title',
                        decoration: const InputDecoration(labelText: 'Sortieren nach'),
                        items: [for (final e in _sorts.entries) DropdownMenuItem(value: e.key, child: Text(e.value))],
                        onChanged: (v) => setState(() {
                          q['sort'] = v;
                          upd();
                        }),
                      ),
                    ),
                    const SizedBox(width: 12),
                    SizedBox(
                      width: 200,
                      child: TextField(
                        controller: limit,
                        keyboardType: TextInputType.number,
                        decoration: const InputDecoration(labelText: 'Höchstzahl (leer = alle)'),
                        onChanged: (v) {
                          q['limit'] = int.tryParse(v.trim());
                          upd();
                        },
                      ),
                    ),
                  ]),
                  const SizedBox(height: 12),
                  Text('Bibliotheken (keine gewählt = alle)', style: TextStyle(color: context.dim)),
                  Wrap(spacing: 8, children: [
                    for (final s in sections)
                      FilterChip(
                        label: Text('${s['title']}'),
                        selected: (q['sections'] as List).contains(int.tryParse('${s['key']}')),
                        onSelected: (on) => setState(() {
                          final id = int.tryParse('${s['key']}');
                          (q['sections'] as List).remove(id);
                          if (on) (q['sections'] as List).add(id);
                          upd();
                        }),
                      ),
                  ]),
                  if (!isDefault) ...[
                    CheckboxListTile(contentPadding: EdgeInsets.zero, controlAffinity: ListTileControlAffinity.leading, title: const Text('Mit allen teilen'), value: shared, onChanged: (v) => setState(() => shared = v == true)),
                    CheckboxListTile(contentPadding: EdgeInsets.zero, controlAffinity: ListTileControlAffinity.leading, title: const Text('Auf der Startseite zeigen'), value: pinned, onChanged: (v) => setState(() => pinned = v == true)),
                  ],
                  const SizedBox(height: 8),
                  Text(preview, style: TextStyle(color: context.dim)),
                  if (error != null && loaded) Padding(padding: const EdgeInsets.only(top: 8), child: Text(error!, style: TextStyle(color: context.bad))),
                ]),
              ),
      ),
      actions: [
        TextButton(onPressed: () => Navigator.pop(context), child: const Text('Abbrechen')),
        FilledButton(onPressed: saving || !loaded ? null : save, child: const Text('Speichern')),
      ],
    );
  }
}
