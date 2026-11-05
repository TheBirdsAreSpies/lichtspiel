import 'dart:convert';
import 'dart:io';
import 'package:flutter/material.dart';
import '../api.dart';

String _list(dynamic v) => ((v as List?) ?? const []).join(', ');

/// Admin: the details of a movie, show, season or episode (title, tags, labels, advanced, pictures).
/// Returns true when something was saved.
Future<bool> editDetailsDialog(BuildContext context, Api api, Map<String, dynamic> it) async {
  List<Map<String, dynamic>> langs = [];
  try {
    final st = await api.get('/api/settings');
    langs = ((st?['languages'] as List?) ?? const []).map((e) => Map<String, dynamic>.from(e)).toList();
  } catch (_) {}
  if (!context.mounted) return false;
  final r = await showDialog<bool>(context: context, builder: (_) => _EditDialog(api: api, it: it, langs: langs));
  return r == true;
}

class _EditDialog extends StatefulWidget {
  final Api api;
  final Map<String, dynamic> it;
  final List<Map<String, dynamic>> langs;
  const _EditDialog({required this.api, required this.it, required this.langs});
  @override
  State<_EditDialog> createState() => _EditDialogState();
}

class _EditDialogState extends State<_EditDialog> {
  final c = <String, TextEditingController>{};
  late String langOverride, credits, useOriginal;
  String? error;
  bool saving = false;

  Map<String, dynamic> get it => widget.it;
  Map<String, dynamic> get x => Map<String, dynamic>.from(it['extra'] ?? const {});
  bool get movie => it['type'] == 'movie' || it['type'] == 'episode';

  @override
  void initState() {
    super.initState();
    void f(String k, dynamic v) => c[k] = TextEditingController(text: v == null ? '' : '$v');
    f('title', it['title']);
    f('sort_title', it['titleSort']);
    f('original_title', x['original_title']);
    f('edition', x['edition']);
    f('originally_available', '${x['originally_available'] ?? ''}'.split('T').first);
    f('year', it['year']);
    f('content_rating', it['contentRating']);
    f('rating', it['rating']);
    f('studio', it['studio']);
    f('tagline', it['tagline']);
    f('summary', it['summary']);
    f('directors', _list(x['directors']));
    f('countries', _list(x['countries']));
    f('genres', _list(it['genres']));
    f('writers', _list(x['writers']));
    f('producers', _list(x['producers']));
    f('collections', _list(x['collections']));
    f('labels', _list(x['labels']));
    langOverride = '${x['language_override'] ?? ''}';
    credits = '${x['credits_detection'] ?? ''}';
    useOriginal = '${x['use_original_title'] ?? ''}';
  }

  Future<void> save() async {
    final body = <String, dynamic>{for (final e in c.entries) e.key: e.value.text};
    body['year'] = c['year']!.text.trim().isEmpty ? null : int.tryParse(c['year']!.text.trim());
    body['rating'] = c['rating']!.text.trim().isEmpty ? null : double.tryParse(c['rating']!.text.trim().replaceAll(',', '.'));
    if (c['sort_title']!.text.trim().isEmpty) body.remove('sort_title');
    if (!movie) body.remove('edition');
    body['language_override'] = langOverride;
    body['credits_detection'] = credits;
    body['use_original_title'] = useOriginal;
    setState(() {
      saving = true;
      error = null;
    });
    try {
      await widget.api.put('/library/metadata/${it['ratingKey']}', body);
      if (mounted) Navigator.pop(context, true);
    } catch (e) {
      if (mounted) {
        setState(() {
          error = '$e';
          saving = false;
        });
      }
    }
  }

  Widget tf(String k, String label, {String? help, int lines = 1, TextInputType? type}) => Padding(
        padding: const EdgeInsets.only(bottom: 12),
        child: TextField(
          controller: c[k],
          minLines: lines,
          maxLines: lines == 1 ? 1 : 8,
          keyboardType: type,
          decoration: InputDecoration(labelText: label, helperText: help, helperMaxLines: 2, border: const OutlineInputBorder()),
        ),
      );

  Widget dd(String label, String value, Map<String, String> opts, void Function(String) on, {String? help}) => Padding(
        padding: const EdgeInsets.only(bottom: 12),
        child: DropdownButtonFormField<String>(
          initialValue: opts.containsKey(value) ? value : '',
          isExpanded: true,
          decoration: InputDecoration(labelText: label, helperText: help, helperMaxLines: 2, border: const OutlineInputBorder()),
          items: [for (final e in opts.entries) DropdownMenuItem(value: e.key, child: Text(e.value))],
          onChanged: (v) => setState(() => on(v ?? '')),
        ),
      );

  @override
  Widget build(BuildContext context) {
    final tabs = ['Allgemein', 'Tags', 'Labels', 'Erweitert', 'Bilder', 'Info'];
    return DefaultTabController(
      length: tabs.length,
      child: AlertDialog(
        title: Text('Details bearbeiten · ${it['title']}'),
        content: SizedBox(
          width: 640,
          height: 520,
          child: Column(children: [
            TabBar(isScrollable: true, tabs: [for (final t in tabs) Tab(text: t)]),
            const SizedBox(height: 12),
            Expanded(
              child: TabBarView(children: [
                ListView(children: [
                  tf('title', 'Titel'),
                  tf('sort_title', 'Sortiertitel', help: 'Leer lassen, um ihn aus dem Titel abzuleiten.'),
                  tf('original_title', 'Originaltitel'),
                  if (movie) tf('edition', 'Edition (z. B. Director’s Cut)'),
                  tf('originally_available', 'Erschienen (JJJJ-MM-TT)'),
                  tf('year', 'Jahr', type: TextInputType.number),
                  tf('content_rating', 'Altersfreigabe'),
                  tf('rating', 'Bewertung (0-10)', type: TextInputType.number),
                  tf('studio', 'Studio'),
                  tf('tagline', 'Tagline'),
                  tf('summary', 'Handlung', lines: 5),
                ]),
                ListView(children: [
                  tf('directors', 'Regie', help: 'Mit Komma trennen'),
                  tf('countries', 'Länder'),
                  tf('genres', 'Genres'),
                  tf('writers', 'Drehbuch'),
                  tf('producers', 'Produktion'),
                  tf('collections', 'Sammlungen'),
                ]),
                ListView(children: [tf('labels', 'Labels (mit Komma getrennt)')]),
                ListView(children: [
                  dd('Sprache der Details', langOverride, {'': 'Standard der Bibliothek', for (final l in widget.langs) '${l['code']}': '${l['name']}'}, (v) => langOverride = v,
                      help: 'Gilt beim nächsten Online-Abruf der Details (Metadaten aktualisieren).'),
                  dd('Abspann erkennen', credits, const {'': 'Standard der Bibliothek', 'on': 'Ja', 'off': 'Nein'}, (v) => credits = v, help: 'Ob für diesen Titel nach der Markierung „Abspann überspringen“ gesucht wird.'),
                  dd('Originaltitel verwenden', useOriginal, const {'': 'Standard der Bibliothek', 'yes': 'Ja', 'no': 'Nein'}, (v) => useOriginal = v, help: 'Zeigt den Originaltitel statt des übersetzten.'),
                ]),
                ListView(children: [
                  _ImagePicker(api: widget.api, it: it, slot: 'poster', label: 'Poster', url: '/library/metadata/${it['ratingKey']}/thumb', has: true),
                  _ImagePicker(api: widget.api, it: it, slot: 'art', label: 'Hintergrund', url: '/library/metadata/${it['ratingKey']}/art', has: true),
                  _ImagePicker(api: widget.api, it: it, slot: 'logo', label: 'Logo', url: it['clearLogo'] as String?, has: it['clearLogo'] != null),
                  _ImagePicker(api: widget.api, it: it, slot: 'square', label: 'Quadratisches Poster', url: it['squareThumb'] as String?, has: it['squareThumb'] != null),
                ]),
                ListView(children: [
                  SelectableText('ID: ${it['ratingKey']}\nArt: ${it['type']}'
                      '${it['addedAt'] != null ? '\nHinzugefügt: ${_at(it['addedAt'])}' : ''}'
                      '${it['updatedAt'] != null ? '\nZuletzt aktualisiert: ${_at(it['updatedAt'])}' : ''}'
                      '${((it['Media'] as List?)?.isNotEmpty ?? false) && it['Media'][0]['file'] != null ? '\nPfad: ${it['Media'][0]['file']}' : ''}'),
                ]),
              ]),
            ),
            if (error != null) Text(error!, style: const TextStyle(color: Colors.redAccent)),
          ]),
        ),
        actions: [
          TextButton(onPressed: () => Navigator.pop(context, false), child: const Text('Abbrechen')),
          FilledButton(onPressed: saving ? null : save, child: const Text('Speichern')),
        ],
      ),
    );
  }

  String _at(dynamic ts) {
    if (ts is! num) return '';
    final d = DateTime.fromMillisecondsSinceEpoch((ts * (ts < 1e12 ? 1000 : 1)).round());
    String two(int n) => n.toString().padLeft(2, '0');
    return '${two(d.day)}.${two(d.month)}.${d.year} ${two(d.hour)}:${two(d.minute)}';
  }
}

/// One picture of an item: shown, replaced from an address or a file (path on this computer), or removed.
class _ImagePicker extends StatefulWidget {
  final Api api;
  final Map<String, dynamic> it;
  final String slot, label;
  final String? url;
  final bool has;
  const _ImagePicker({required this.api, required this.it, required this.slot, required this.label, required this.url, required this.has});
  @override
  State<_ImagePicker> createState() => _ImagePickerState();
}

class _ImagePickerState extends State<_ImagePicker> {
  late bool has = widget.has;
  late String? url = widget.url;
  int stamp = DateTime.now().millisecondsSinceEpoch;
  final addr = TextEditingController();

  String get path => '/library/metadata/${widget.it['ratingKey']}/image/${widget.slot}';

  void say(String s) => ScaffoldMessenger.of(context).showSnackBar(SnackBar(content: Text(s)));

  Future<void> send(Map<String, dynamic> body) async {
    try {
      await widget.api.put(path, body);
      setState(() {
        url ??= '/library/metadata/${widget.it['ratingKey']}/${widget.slot == 'poster' ? 'thumb' : widget.slot}';
        has = true;
        stamp = DateTime.now().millisecondsSinceEpoch;
      });
      say('Bild gespeichert');
    } catch (e) {
      say('$e');
    }
  }

  Future<void> fromFile() async {
    final p = TextEditingController();
    final ok = await showDialog<bool>(
      context: context,
      builder: (d) => AlertDialog(
        title: const Text('Bild aus Datei'),
        content: TextField(controller: p, autofocus: true, decoration: const InputDecoration(labelText: 'Pfad zur Bilddatei', hintText: r'C:\Bilder\poster.jpg')),
        actions: [TextButton(onPressed: () => Navigator.pop(d, false), child: const Text('Abbrechen')), FilledButton(onPressed: () => Navigator.pop(d, true), child: const Text('Hochladen'))],
      ),
    );
    if (ok != true) return;
    try {
      final f = File(p.text.trim().replaceAll('"', ''));
      final bytes = await f.readAsBytes();
      if (bytes.length > 3 * 1024 * 1024) return say('Das Bild ist zu groß (höchstens 3 MB)');
      final ext = f.path.toLowerCase().split('.').last;
      final mime = {'png': 'image/png', 'webp': 'image/webp', 'gif': 'image/gif'}[ext] ?? 'image/jpeg';
      await send({'data': 'data:$mime;base64,${base64Encode(bytes)}'});
    } catch (e) {
      say('$e');
    }
  }

  @override
  Widget build(BuildContext context) {
    final u = url;
    return Padding(
      padding: const EdgeInsets.only(bottom: 20),
      child: Column(crossAxisAlignment: CrossAxisAlignment.start, children: [
        Text(widget.label, style: Theme.of(context).textTheme.titleSmall),
        const SizedBox(height: 6),
        if (has && u != null)
          ConstrainedBox(
            constraints: const BoxConstraints(maxHeight: 200),
            child: Image.network('${widget.api.abs(u)}&t=$stamp', errorBuilder: (_, __, ___) => const Text('Kein Bild', style: TextStyle(color: Colors.white54))),
          )
        else
          const Text('Kein Bild', style: TextStyle(color: Colors.white54)),
        const SizedBox(height: 6),
        Wrap(spacing: 8, crossAxisAlignment: WrapCrossAlignment.center, children: [
          OutlinedButton(onPressed: fromFile, child: const Text('Aus Datei')),
          SizedBox(width: 260, child: TextField(controller: addr, decoration: const InputDecoration(isDense: true, hintText: 'https://…', border: OutlineInputBorder()))),
          OutlinedButton(onPressed: () => addr.text.trim().isEmpty ? null : send({'url': addr.text.trim()}), child: const Text('Adresse nutzen')),
          TextButton(
            onPressed: () async {
              try {
                await widget.api.delete(path);
                setState(() => has = false);
                say('Bild entfernt');
              } catch (e) {
                say('$e');
              }
            },
            child: const Text('Entfernen', style: TextStyle(color: Colors.redAccent)),
          ),
        ]),
      ]),
    );
  }
}

/// Admin: look up the title online and pick the right match. Returns true when a match was applied.
Future<bool> matchDialog(BuildContext context, Api api, Map<String, dynamic> it) async {
  final r = await showDialog<bool>(context: context, builder: (_) => _MatchDialog(api: api, it: it));
  return r == true;
}

class _MatchDialog extends StatefulWidget {
  final Api api;
  final Map<String, dynamic> it;
  const _MatchDialog({required this.api, required this.it});
  @override
  State<_MatchDialog> createState() => _MatchDialogState();
}

class _MatchDialogState extends State<_MatchDialog> {
  late final title = TextEditingController(text: '${widget.it['extra']?['file_title'] ?? widget.it['title']}');
  late final year = TextEditingController(text: '${widget.it['year'] ?? ''}');
  List<Map<String, dynamic>>? matches;
  String? message;
  bool busy = false;

  @override
  void initState() {
    super.initState();
    search();
  }

  Future<void> search() async {
    setState(() {
      busy = true;
      message = null;
      matches = null;
    });
    try {
      final r = await widget.api.get('/library/metadata/${widget.it['ratingKey']}/matches?title=${Uri.encodeQueryComponent(title.text)}&year=${Uri.encodeQueryComponent(year.text)}');
      final l = ((r?['matches'] as List?) ?? const []).map((e) => Map<String, dynamic>.from(e)).toList();
      if (mounted) {
        setState(() {
          matches = l;
          busy = false;
          if (l.isEmpty) message = 'Nichts gefunden. Probier eine andere Schreibweise oder lass das Jahr leer.';
        });
      }
    } catch (e) {
      if (mounted) {
        setState(() {
          busy = false;
          message = '$e';
        });
      }
    }
  }

  Future<void> pick(Map<String, dynamic> m) async {
    setState(() {
      busy = true;
      message = 'Lade Details für ${m['title']}…';
    });
    try {
      final r = await widget.api.put('/library/metadata/${widget.it['ratingKey']}/match', {'source': m['source'], 'id': m['id']});
      if (mounted) {
        if (r is Map && r['message'] != null) ScaffoldMessenger.of(context).showSnackBar(SnackBar(content: Text('${r['message']}')));
        Navigator.pop(context, true);
      }
    } catch (e) {
      if (mounted) {
        setState(() {
          busy = false;
          message = '$e';
        });
      }
    }
  }

  @override
  Widget build(BuildContext context) {
    final l = matches;
    return AlertDialog(
      title: Text(widget.it['extra']?['metadata_source'] != null ? 'Zuordnung korrigieren' : 'Zuordnen'),
      content: SizedBox(
        width: 620,
        height: 520,
        child: Column(children: [
          Row(children: [
            Expanded(child: TextField(controller: title, autofocus: true, decoration: const InputDecoration(labelText: 'Titel'), onSubmitted: (_) => search())),
            const SizedBox(width: 12),
            SizedBox(width: 90, child: TextField(controller: year, keyboardType: TextInputType.number, decoration: const InputDecoration(labelText: 'Jahr'), onSubmitted: (_) => search())),
            const SizedBox(width: 12),
            FilledButton(onPressed: busy ? null : search, child: const Text('Suchen')),
          ]),
          const SizedBox(height: 8),
          if (busy && l == null) const Padding(padding: EdgeInsets.all(16), child: CircularProgressIndicator()),
          if (message != null) Padding(padding: const EdgeInsets.all(8), child: Text(message!, style: const TextStyle(color: Colors.white70))),
          if (l != null)
            Expanded(
              child: ListView(children: [
                for (final m in l)
                  ListTile(
                    enabled: !busy,
                    leading: m['thumb'] != null && '${m['thumb']}'.isNotEmpty
                        ? Image.network('${m['thumb']}', width: 46, height: 68, fit: BoxFit.cover, errorBuilder: (_, __, ___) => const SizedBox(width: 46))
                        : CircleAvatar(child: Text('${m['title']}'.isEmpty ? '?' : '${m['title']}'[0])),
                    title: Text('${m['title']}${m['year'] != null ? ' (${m['year']})' : ''}'),
                    subtitle: Text(
                      '${m['source']} · Treffer ${((((m['score'] ?? 0) as num).clamp(0, 1)) * 100).round()} %${'${m['summary'] ?? ''}'.isEmpty ? '' : '\n${m['summary']}'}',
                      maxLines: 3,
                      overflow: TextOverflow.ellipsis,
                    ),
                    isThreeLine: '${m['summary'] ?? ''}'.isNotEmpty,
                    onTap: () => pick(m),
                  ),
              ]),
            ),
        ]),
      ),
      actions: [TextButton(onPressed: () => Navigator.pop(context, false), child: const Text('Schließen'))],
    );
  }
}
