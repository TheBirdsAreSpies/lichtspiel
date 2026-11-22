import 'dart:convert';
import 'dart:isolate';
import 'package:flutter/material.dart';
import 'package:image_picker/image_picker.dart';
import '../api.dart';
import '../picture.dart';
import '../theme.dart';
import '../i18n.dart';

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
    final names = {
      'manual': tr('set by hand'),
      'chapters': tr('from a chapter “Credits” in the file'),
      'picture': tr('found in the picture (a dark screen with text at the end)'),
      'sound': tr('found by comparing the sound of the episodes'),
      'none': tr('none found'),
      'pending': tr('not searched yet (the search runs in the background)'),
    };
    final n = errors is num && errors > 0 ? tr('; search failed {errors}×', {'errors': errors.toInt()}) : '';
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
          if (ms == null) return setState(() => error = tr('Credits: please enter a time like 1:52:30.'));
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
          if (a == null || b == null || b <= a) return setState(() => error = tr('Intro: give its start and end, the end after the start.'));
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
          tr('Intro: {source}.\nCredits: {source2}.{at}', {'source': _source(m['introSource']), 'source2': _source(m['source'], errors: m['errors']), 'at': at.isEmpty ? '' : tr(' The video counts as watched from {at} of {len}.', {'at': at, 'len': len})}),
          style: TextStyle(color: context.soft),
        ),
      ),
      mf('is', tr('Intro starts at (h:min:s)')),
      mf('ie', tr('Intro ends at'), help: tr('Leave both empty to go back to the found intro.')),
      mf('cs', tr('Credits start at'), help: tr("Leave empty to use the file's chapters again.")),
      Align(
        alignment: Alignment.centerLeft,
        child: OutlinedButton(
          onPressed: () async {
            final msg = ScaffoldMessenger.of(context);
            try {
              await widget.api.post('/library/metadata/${it['ratingKey']}/credits');
              msg.showSnackBar(SnackBar(content: Text(tr('Detecting credits…'))));
            } catch (e) {
              msg.showSnackBar(SnackBar(content: Text('$e')));
            }
          },
          child: Text(tr('Detect credits again')),
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
    final tabs = [tr('General'), tr('Tags'), tr('Labels'), if (mk != null) tr('Markers'), tr('Advanced'), tr('Pictures'), tr('Info')];
    return DefaultTabController(
      length: tabs.length,
      child: AlertDialog(
        title: Text(tr('Edit details · {it}', {'it': it['title']})),
        content: SizedBox(
          width: 640,
          height: 520,
          child: Column(children: [
            TabBar(isScrollable: true, tabs: [for (final t in tabs) Tab(text: t)]),
            const SizedBox(height: 12),
            Expanded(
              child: TabBarView(children: [
                ListView(children: [
                  tf('title', tr('Title')),
                  tf('sort_title', tr('Sort title'), help: tr('Leave empty to derive it from the title.')),
                  tf('original_title', tr('Original title')),
                  if (movie) tf('edition', tr('Edition (e.g. Director’s Cut)')),
                  tf('originally_available', tr('Released (YYYY-MM-DD)')),
                  tf('year', tr('Year'), type: TextInputType.number),
                  tf('content_rating', tr('Age rating')),
                  tf('rating', tr('Rating (0-10)'), type: TextInputType.number),
                  tf('studio', 'Studio'),
                  tf('tagline', tr('Tagline')),
                  tf('summary', tr('Plot'), lines: 5),
                ]),
                ListView(children: [
                  tf('directors', tr('Director'), help: tr('Separate with commas')),
                  tf('countries', tr('Countries')),
                  tf('genres', tr('Genres')),
                  tf('writers', tr('Writer')),
                  tf('producers', tr('Producers')),
                  tf('collections', tr('Collections')),
                ]),
                ListView(children: [tf('labels', tr('Labels (separated by commas)'))]),
                if (mk != null) markerTab(),
                ListView(children: [
                  dd(tr('Details language'), langOverride, {'': tr('Library default'), for (final l in widget.langs) '${l['code']}': '${l['name']}'}, (v) => langOverride = v,
                      help: tr('Applies the next time the details are fetched online (Refresh metadata).')),
                  dd(tr('Detect credits'), credits, {'': tr('Library default'), 'on': tr('Yes'), 'off': tr('No')}, (v) => credits = v, help: tr('Whether this title is searched for the “Skip credits” marker.')),
                  dd(tr('Use the original title'), useOriginal, {'': tr('Library default'), 'yes': tr('Yes'), 'no': tr('No')}, (v) => useOriginal = v, help: tr('Shows the original title instead of the translated one.')),
                ]),
                ListView(children: [
                  _ImagePicker(api: widget.api, it: it, slot: 'poster', label: tr('Poster'), url: '/library/metadata/${it['ratingKey']}/thumb', has: true),
                  _ImagePicker(api: widget.api, it: it, slot: 'art', label: tr('Background'), url: '/library/metadata/${it['ratingKey']}/art', has: true),
                  _ImagePicker(api: widget.api, it: it, slot: 'logo', label: tr('Logo'), url: it['clearLogo'] as String?, has: it['clearLogo'] != null),
                  _ImagePicker(api: widget.api, it: it, slot: 'square', label: tr('Square poster'), url: it['squareThumb'] as String?, has: it['squareThumb'] != null),
                ]),
                ListView(children: [
                  SelectableText(tr('ID: {it}\nType: {it2}{it3}{it4}{x}', {'it': it['ratingKey'], 'it2': it['type'], 'it3': it['addedAt'] != null ? tr('\nAdded: {at}', {'at': _at(it['addedAt'])}) : '', 'it4': it['updatedAt'] != null ? tr('\nLast updated: {at}', {'at': _at(it['updatedAt'])}) : '', 'x': ((it['Media'] as List?)?.isNotEmpty ?? false) && it['Media'][0]['file'] != null ? tr('\nPath: {it}', {'it': it['Media'][0]['file']}) : ''})),
                ]),
              ]),
            ),
            if (error != null) Text(error!, style: TextStyle(color: context.bad)),
          ]),
        ),
        actions: [
          TextButton(onPressed: () => Navigator.pop(context, false), child: Text(tr('Cancel'))),
          FilledButton(onPressed: saving ? null : save, child: Text(tr('Save'))),
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
      say(tr('Picture saved'));
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
      if (bytes.length > maxPictureBytes) say(tr('Making the picture smaller…'));
      final pic = bytes.length <= maxPictureBytes ? (bytes: bytes, mime: mime) : await Isolate.run(() => shrinkPicture(bytes, mime: mime));
      if (pic == null) return say(tr('The picture is too large (3 MB at most) and could not be made smaller'));
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
            child: Image.network('${widget.api.abs(u)}${u.contains('?') ? '&' : '?'}t=$stamp', headers: widget.api.auth, errorBuilder: (_, _, _) => Text(tr('No picture'), style: TextStyle(color: context.faint))),
          )
        else
          Text(tr('No picture'), style: TextStyle(color: context.faint)),
        const SizedBox(height: 6),
        Wrap(spacing: 8, crossAxisAlignment: WrapCrossAlignment.center, children: [
          OutlinedButton(onPressed: fromFile, child: Text(tr('From a file'))),
          ConstrainedBox(constraints: const BoxConstraints(maxWidth: 260), child: TextField(controller: addr, decoration: const InputDecoration(isDense: true, hintText: 'https://…', border: OutlineInputBorder()))),
          OutlinedButton(onPressed: () => addr.text.trim().isEmpty ? null : send({'url': addr.text.trim()}), child: Text(tr('Use the address'))),
          TextButton(
            onPressed: () async {
              try {
                await widget.api.delete(path);
                setState(() => has = false);
                say(tr('Picture removed'));
              } catch (e) {
                say('$e');
              }
            },
            child: Text(tr('Remove'), style: TextStyle(color: context.bad)),
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
          if (l.isEmpty) message = tr('Nothing found. Try another spelling or leave the year empty.');
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
      message = tr('Loading details for {m}…', {'m': m['title']});
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
      title: Text(widget.it['extra']?['metadata_source'] != null ? tr('Fix match') : tr('Match')),
      content: SizedBox(
        width: 620,
        height: 520,
        child: Column(children: [
          Row(children: [
            Expanded(child: TextField(controller: title, autofocus: true, decoration: InputDecoration(labelText: tr('Title')), onSubmitted: (_) => search())),
            const SizedBox(width: 12),
            SizedBox(width: 90, child: TextField(controller: year, keyboardType: TextInputType.number, decoration: InputDecoration(labelText: tr('Year')), onSubmitted: (_) => search())),
            const SizedBox(width: 12),
            FilledButton(onPressed: busy ? null : search, child: Text(tr('Search'))),
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
                      tr('{m} · match {x} %{x2}', {'m': m['source'], 'x': ((((m['score'] ?? 0) as num).clamp(0, 1)) * 100).round(), 'x2': '${m['summary'] ?? ''}'.isEmpty ? '' : '\n${m['summary']}'}),
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
      actions: [TextButton(onPressed: () => Navigator.pop(context, false), child: Text(tr('Close')))],
    );
  }
}
