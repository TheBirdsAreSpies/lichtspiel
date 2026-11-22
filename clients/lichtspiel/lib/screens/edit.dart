import 'dart:convert';
import 'dart:isolate';
import 'package:flutter/material.dart';
import 'package:image_picker/image_picker.dart';
import '../api.dart';
import '../picture.dart';
import '../theme.dart';

String _list(dynamic v) => ((v as List?) ?? const []).join(', ');

/// Admin: the details of a movie, show, season or episode (title, tags, labels, advanced, pictures).
/// Returns true when something was saved.
Future<bool> editDetailsDialog(BuildContext context, Api api, Map<String, dynamic> it) async {
  List<Map<String, dynamic>> langs = [];
  try {
    final st = await api.get('/api/settings');
    langs = ((st?['languages'] as List?) ?? const []).map((e) => Map<String, dynamic>.from(e)).toList();
  } catch (_) {}
  // Intro and credits markers exist for movies and episodes only.
  Map<String, dynamic>? markers;
  if (it['type'] == 'movie' || it['type'] == 'episode') {
    try {
      final m = await api.get('/library/metadata/${it['ratingKey']}/markers');
      if (m is Map) markers = Map<String, dynamic>.from(m);
    } catch (_) {}
  }
  if (!context.mounted) return false;
  final r = await showDialog<bool>(context: context, builder: (_) => _EditDialog(api: api, it: it, langs: langs, markers: markers));
  return r == true;
}

class _EditDialog extends StatefulWidget {
  final Api api;
  final Map<String, dynamic> it;
  final List<Map<String, dynamic>> langs;
  final Map<String, dynamic>? markers;
  const _EditDialog({required this.api, required this.it, required this.langs, this.markers});
  @override
  State<_EditDialog> createState() => _EditDialogState();
}

class _EditDialogState extends State<_EditDialog> {
  final c = <String, TextEditingController>{};
  late String langOverride, credits, useOriginal;
  String? error;
  bool saving = false;
  // Markers as h:mm:ss; what was there at the start tells which ones were changed.
  final mc = <String, TextEditingController>{};
  final mInit = <String, String>{};

  Map<String, dynamic>? get mk => widget.markers;

  static String _clock(dynamic ms) {
    if (ms is! num) return '';
    final s = (ms / 1000).floor();
    String two(int n) => n.toString().padLeft(2, '0');
    return '${s ~/ 3600}:${two((s ~/ 60) % 60)}:${two(s % 60)}';
  }

  /// "1:52:30", "52:30" or "90" to ms; null when it isn't a time.
  static int? _unclock(String v) {
    final t = v.trim();
    if (t.isEmpty) return null;
    var total = 0;
    for (final p in t.split(':')) {
      final n = int.tryParse(p.trim());
      if (n == null || n < 0) return null;
      total = total * 60 + n;
    }
    return total * 1000;
  }

  String _source(dynamic src, {dynamic errors}) {
    const names = {
      'manual': 'von Hand gesetzt',
      'chapters': 'aus einem Kapitel „Credits“ in der Datei',
      'picture': 'im Bild gefunden (dunkler Bildschirm mit Text am Ende)',
      'sound': 'durch Vergleich des Tons der Folgen gefunden',
      'none': 'nichts gefunden',
      'pending': 'noch nicht gesucht (die Suche läuft im Hintergrund)',
    };
    final n = errors is num && errors > 0 ? '; Suche ${errors.toInt()}× fehlgeschlagen' : '';
    return '${names['$src'] ?? src}$n';
  }

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
    final m = mk;
    if (m != null) {
      final cr = (m['credits'] as List?) ?? const [];
      final intro = (m['intro'] as List?) ?? const [];
      mInit['cs'] = cr.isEmpty ? '' : _clock(cr[0]);
      mInit['is'] = intro.length == 2 ? _clock(intro[0]) : '';
      mInit['ie'] = intro.length == 2 ? _clock(intro[1]) : '';
      for (final e in mInit.entries) {
        mc[e.key] = TextEditingController(text: e.value);
      }
    }
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
    // Only changed markers are sent; emptied ones go back to what was found (only a hand-set one can be taken back).
    final mb = <String, dynamic>{};
    final m = mk;
    if (m != null) {
      final cs = mc['cs']!.text.trim(), isv = mc['is']!.text.trim(), ie = mc['ie']!.text.trim();
      if (cs != mInit['cs']) {
        if (cs.isNotEmpty) {
          final ms = _unclock(cs);
          if (ms == null) return setState(() => error = 'Abspann: bitte eine Zeit wie 1:52:30 eingeben.');
          mb['credits'] = [ms];
        } else if (m['source'] == 'manual') {
          mb['credits'] = null;
        }
      }
      if (isv != mInit['is'] || ie != mInit['ie']) {
        if (isv.isEmpty && ie.isEmpty) {
          if (m['introSource'] == 'manual') mb['intro'] = null;
        } else {
          final a = _unclock(isv), b = _unclock(ie);
          if (a == null || b == null || b <= a) return setState(() => error = 'Intro: Anfang und Ende angeben, das Ende nach dem Anfang.');
          mb['intro'] = [a, b];
        }
      }
    }
    setState(() {
      saving = true;
      error = null;
    });
    try {
      await widget.api.put('/library/metadata/${it['ratingKey']}', body);
      if (mb.isNotEmpty) await widget.api.put('/library/metadata/${it['ratingKey']}/markers', mb);
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

  Widget mf(String k, String label, {String? help}) => Padding(
        padding: const EdgeInsets.only(bottom: 12),
        child: TextField(
          controller: mc[k],
          decoration: InputDecoration(labelText: label, helperText: help, helperMaxLines: 2, hintText: 'h:mm:ss', border: const OutlineInputBorder()),
        ),
      );

  Widget markerTab() {
    final m = mk!;
    final at = m['watchedAt'] is num ? _clock(m['watchedAt']) : '';
    final len = m['duration'] is num ? _clock(m['duration']) : '';
    return ListView(children: [
      Padding(
        padding: const EdgeInsets.only(bottom: 12),
        child: Text(
          'Intro: ${_source(m['introSource'])}.\nAbspann: ${_source(m['source'], errors: m['errors'])}.'
          '${at.isEmpty ? '' : ' Das Video gilt ab $at von $len als gesehen.'}',
          style: TextStyle(color: context.soft),
        ),
      ),
      mf('is', 'Intro beginnt bei (Std:Min:Sek)'),
      mf('ie', 'Intro endet bei', help: 'Beide leer lassen, um zum erkannten Intro zurückzukehren.'),
      mf('cs', 'Abspann beginnt bei', help: 'Leer lassen, um wieder die Kapitel der Datei zu verwenden.'),
      Align(
        alignment: Alignment.centerLeft,
        child: OutlinedButton(
          onPressed: () async {
            final msg = ScaffoldMessenger.of(context);
            try {
              await widget.api.post('/library/metadata/${it['ratingKey']}/credits');
              msg.showSnackBar(const SnackBar(content: Text('Abspann wird gesucht …')));
            } catch (e) {
              msg.showSnackBar(SnackBar(content: Text('$e')));
            }
          },
          child: const Text('Abspann erneut suchen'),
        ),
      ),
    ]);
  }

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
    final tabs = ['Allgemein', 'Tags', 'Labels', if (mk != null) 'Marker', 'Erweitert', 'Bilder', 'Info'];
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
                if (mk != null) markerTab(),
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
            if (error != null) Text(error!, style: TextStyle(color: context.bad)),
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

/// One picture of an item: shown, replaced from an address or a picked file, or removed.
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

  /// Picks a picture with the system's picker (photo library on iPhone/iPad, a file dialog elsewhere).
  Future<void> fromFile() async {
    try {
      final f = await ImagePicker().pickImage(source: ImageSource.gallery, maxWidth: 3000, maxHeight: 3000, imageQuality: 92, requestFullMetadata: false);
      if (f == null) return;
      final bytes = await f.readAsBytes();
      final ext = f.name.toLowerCase().split('.').last;
      final mime = f.mimeType ?? {'png': 'image/png', 'webp': 'image/webp', 'gif': 'image/gif'}[ext] ?? 'image/jpeg';
      // On a computer the picker can't shrink the picture (image_picker ignores maxWidth/imageQuality
      // there), so a big one is shrunk here, away from the screen's thread.
      if (bytes.length > maxPictureBytes) say('Das Bild wird verkleinert…');
      final pic = bytes.length <= maxPictureBytes ? (bytes: bytes, mime: mime) : await Isolate.run(() => shrinkPicture(bytes, mime: mime));
      if (pic == null) return say('Das Bild ist zu groß (höchstens 3 MB) und ließ sich nicht verkleinern');
      await send({'data': 'data:${pic.mime};base64,${base64Encode(pic.bytes)}'});
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
            child: Image.network('${widget.api.abs(u)}${u.contains('?') ? '&' : '?'}t=$stamp', headers: widget.api.auth, errorBuilder: (_, _, _) => Text('Kein Bild', style: TextStyle(color: context.faint))),
          )
        else
          Text('Kein Bild', style: TextStyle(color: context.faint)),
        const SizedBox(height: 6),
        Wrap(spacing: 8, crossAxisAlignment: WrapCrossAlignment.center, children: [
          OutlinedButton(onPressed: fromFile, child: const Text('Aus Datei')),
          ConstrainedBox(constraints: const BoxConstraints(maxWidth: 260), child: TextField(controller: addr, decoration: const InputDecoration(isDense: true, hintText: 'https://…', border: OutlineInputBorder()))),
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
            child: Text('Entfernen', style: TextStyle(color: context.bad)),
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
      final r = await widget.api.get('/library/metadata/${widget.it['ratingKey']}/matches?title=${Uri.encodeQueryComponent(title.text)}&year=${Uri.encodeQueryComponent(year.text)}', slow: true);
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
          if (message != null) Padding(padding: const EdgeInsets.all(8), child: Text(message!, style: TextStyle(color: context.soft))),
          if (l != null)
            Expanded(
              child: ListView(children: [
                for (final m in l)
                  ListTile(
                    enabled: !busy,
                    leading: m['thumb'] != null && '${m['thumb']}'.isNotEmpty
                        ? Image.network('${m['thumb']}', width: 46, height: 68, fit: BoxFit.cover, errorBuilder: (_, _, _) => const SizedBox(width: 46))
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
