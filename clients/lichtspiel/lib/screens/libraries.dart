import 'dart:async';
import 'package:flutter/material.dart';
import '../api.dart';
import '../theme.dart';
import '../i18n.dart';

/// Admin: the libraries with scanning, detection and folders, and adding a new one.
class LibrariesScreen extends StatefulWidget {
  final Api api;
  const LibrariesScreen({super.key, required this.api});
  @override
  State<LibrariesScreen> createState() => _LibrariesScreenState();
}

class _LibrariesScreenState extends State<LibrariesScreen> {
  List<Map<String, dynamic>> secs = [];
  List<Map<String, dynamic>> langs = [];
  String serverLang = '';
  String? error;
  bool loaded = false;
  Timer? poll;

  Api get api => widget.api;

  static Map<String, String> get kinds => {'movie': tr('Movies'), 'show': tr('Shows'), 'music': tr('Music'), 'other': tr('Other videos')};
  static Map<String, String> get intervals => {
    '': tr('Same as the server'),
    '0': tr('Never by itself'),
    '15': tr('every 15 minutes'),
    '30': tr('every 30 minutes'),
    '60': tr('hourly'),
    '120': tr('every 2 hours'),
    '360': tr('every 6 hours'),
    '720': tr('every 12 hours'),
    '1440': tr('daily'),
  };

  @override
  void initState() {
    super.initState();
    init();
  }

  @override
  void dispose() {
    poll?.cancel();
    super.dispose();
  }

  Future<void> init() async {
    try {
      final st = await api.get('/api/settings');
      langs = ((st?['languages'] as List?) ?? const []).map((e) => Map<String, dynamic>.from(e)).toList();
      final code = st?['settings']?['metadata_language'];
      serverLang = '${langs.firstWhere((l) => l['code'] == code, orElse: () => {'name': code ?? ''})['name']}';
    } catch (_) {}
    await load();
  }

  Future<void> load() async {
    try {
      final r = await api.sections();
      if (!mounted) return;
      setState(() {
        secs = r;
        loaded = true;
      });
      poll?.cancel();
      if (r.any((s) => s['scanning'] == true)) poll = Timer(const Duration(seconds: 2), load);
    } catch (e) {
      if (mounted) setState(() => error = '$e');
    }
  }

  void say(String s) => ScaffoldMessenger.of(context).showSnackBar(SnackBar(content: Text(s)));

  Future<void> run(Future<dynamic> Function() f, {String? ok}) async {
    try {
      final r = await f();
      if (ok != null) say(ok);
      await load();
      return r;
    } catch (e) {
      say('$e');
    }
  }

  String when(dynamic s) {
    if (s is! num || s <= 0) return tr('never');
    final d = DateTime.fromMillisecondsSinceEpoch((s * 1000).round());
    String two(int n) => n.toString().padLeft(2, '0');
    return '${two(d.day)}.${two(d.month)}.${d.year} ${two(d.hour)}:${two(d.minute)}';
  }

  Future<void> addDialog() async {
    final name = TextEditingController(), paths = TextEditingController();
    var kind = 'movie', lang = '';
    final ok = await showDialog<bool>(
      context: context,
      builder: (d) => StatefulBuilder(
        builder: (d, set) => AlertDialog(
          title: Text(tr('Add a library')),
          content: SizedBox(
            width: 460,
            child: SingleChildScrollView(
              child: Column(mainAxisSize: MainAxisSize.min, crossAxisAlignment: CrossAxisAlignment.start, children: [
                TextField(controller: name, decoration: InputDecoration(labelText: tr('Name'), hintText: tr('Movies'))),
                const SizedBox(height: 12),
                DropdownButtonFormField<String>(
                  isExpanded: true,
                  initialValue: kind,
                  decoration: InputDecoration(labelText: tr('Kind')),
                  items: [for (final e in kinds.entries) DropdownMenuItem(value: e.key, child: Text(e.value))],
                  onChanged: (v) => set(() => kind = v ?? 'movie'),
                ),
                const SizedBox(height: 12),
                DropdownButtonFormField<String>(
                  isExpanded: true,
                  initialValue: lang,
                  decoration: InputDecoration(labelText: tr('Language of titles and plots')),
                  items: [DropdownMenuItem(value: '', child: Text(tr('Same as the server ({serverLang})', {'serverLang': serverLang}))), for (final l in langs) DropdownMenuItem(value: '${l['code']}', child: Text('${l['name']}'))],
                  onChanged: (v) => set(() => lang = v ?? ''),
                ),
                const SizedBox(height: 12),
                TextField(
                  controller: paths,
                  minLines: 3,
                  maxLines: 6,
                  decoration: InputDecoration(labelText: tr('Folder'), hintText: tr('One folder per line, e.g. D:\\Movies or /volume1/video/Movies'), border: OutlineInputBorder()),
                ),
              ]),
            ),
          ),
          actions: [TextButton(onPressed: () => Navigator.pop(d, false), child: Text(tr('Cancel'))), FilledButton(onPressed: () => Navigator.pop(d, true), child: Text(tr('Add')))],
        ),
      ),
    );
    if (ok != true) return;
    run(() => api.post('/library/sections', {'name': name.text, 'type': kind, 'paths': paths.text, 'language': lang}), ok: tr('Library added, being scanned'));
  }

  Future<void> foldersDialog(Map<String, dynamic> s) async {
    final c = TextEditingController(text: ((s['paths'] as List?) ?? const []).join('\n'));
    final ok = await showDialog<bool>(
      context: context,
      builder: (d) => AlertDialog(
        title: Text(tr('Folders of {s}', {'s': s['title']})),
        content: SizedBox(width: 460, child: TextField(controller: c, minLines: 3, maxLines: 8, decoration: InputDecoration(helperText: tr('One folder per line'), border: OutlineInputBorder()))),
        actions: [TextButton(onPressed: () => Navigator.pop(d, false), child: Text(tr('Cancel'))), FilledButton(onPressed: () => Navigator.pop(d, true), child: Text(tr('Save')))],
      ),
    );
    if (ok != true) return;
    run(() => api.put('/library/sections/${s['key']}', {'paths': c.text.split('\n').map((e) => e.trim()).where((e) => e.isNotEmpty).toList()}), ok: tr('Saved'));
  }

  Future<void> remove(Map<String, dynamic> s) async {
    final c = TextEditingController();
    final ok = await showDialog<bool>(
      context: context,
      builder: (d) => AlertDialog(
        title: Text(tr('Remove the library “{s}”?', {'s': s['title']})),
        content: Column(mainAxisSize: MainAxisSize.min, crossAxisAlignment: CrossAxisAlignment.start, children: [
          Text(tr('Your files stay on the disk. Type the name to confirm:')),
          TextField(controller: c, autofocus: true),
        ]),
        actions: [
          TextButton(onPressed: () => Navigator.pop(d, false), child: Text(tr('Cancel'))),
          FilledButton(onPressed: () => Navigator.pop(d, c.text.trim() == '${s['title']}'), child: Text(tr('Remove'))),
        ],
      ),
    );
    if (ok == true) run(() => api.delete('/library/sections/${s['key']}'));
  }

  Widget card(Map<String, dynamic> s) {
    final id = '${s['key']}';
    final type = '${s['type']}';
    final interval = s['scanIntervalMinutes'] == null ? '' : '${s['scanIntervalMinutes']}';
    return Card(
      child: Padding(
        padding: const EdgeInsets.all(12),
        child: Column(crossAxisAlignment: CrossAxisAlignment.start, children: [
          Row(children: [
            Text('${s['title']}', style: const TextStyle(fontWeight: FontWeight.bold, fontSize: 16)),
            const SizedBox(width: 8),
            Chip(label: Text(kinds[type] ?? type), visualDensity: VisualDensity.compact),
            if (s['scanning'] == true) Padding(padding: EdgeInsets.only(left: 8), child: Text(tr('scanning…'), style: TextStyle(color: context.gold))),
          ]),
          Text(tr('{type}{s} videos · last scan: {when}', {'type': type == 'show' ? tr('{s} shows,', {'s': s['count']}) : '', 's': s['videoCount'] ?? 0, 'when': when(s['scannedAt'])}), style: TextStyle(color: context.dim)),
          Text(((s['paths'] as List?) ?? const []).join(' · '), style: TextStyle(fontSize: 12, color: context.ghost)),
          Wrap(spacing: 20, crossAxisAlignment: WrapCrossAlignment.center, children: [
            Row(mainAxisSize: MainAxisSize.min, children: [
              Checkbox(value: s['creditsDetection'] != false, onChanged: (v) => run(() => api.put('/library/sections/$id', {'credits_detection': v}), ok: tr('Saved'))),
              Text(tr('Detect credits')),
            ]),
            if (type == 'show')
              Row(mainAxisSize: MainAxisSize.min, children: [
                Checkbox(value: s['introDetection'] != false, onChanged: (v) => run(() => api.put('/library/sections/$id', {'intro_detection': v}), ok: tr('Saved'))),
                Text(tr('Detect intros')),
              ]),
            Row(mainAxisSize: MainAxisSize.min, children: [
              Text(tr('Scan interval  ')),
              DropdownButton<String>(
                value: intervals.containsKey(interval) ? interval : '',
                items: [for (final e in intervals.entries) DropdownMenuItem(value: e.key, child: Text(e.value))],
                onChanged: (v) => run(() => api.put('/library/sections/$id', {'scan_interval_minutes': (v ?? '').isEmpty ? null : int.parse(v!)}), ok: tr('Saved')),
              ),
            ]),
            Row(mainAxisSize: MainAxisSize.min, children: [
              Text(tr('Details language  ')),
              DropdownButton<String>(
                value: langs.any((l) => l['code'] == s['language']) ? '${s['language']}' : '',
                items: [DropdownMenuItem(value: '', child: Text(tr('Same as the server ({serverLang})', {'serverLang': serverLang}))), for (final l in langs) DropdownMenuItem(value: '${l['code']}', child: Text('${l['name']}'))],
                onChanged: (v) async {
                  try {
                    final r = await api.put('/library/sections/$id', {'language': v ?? ''});
                    say(r is Map && r['refetching'] != null && r['refetching'] != 0 ? tr('Details for {r} titles are being reloaded', {'r': r['refetching']}) : tr('Saved'));
                    load();
                  } catch (e) {
                    say('$e');
                  }
                },
              ),
            ]),
          ]),
          Wrap(spacing: 8, runSpacing: 4, children: [
            OutlinedButton(onPressed: () => run(() => api.post('/library/sections/$id/refresh'), ok: tr('Scanning {s}', {'s': s['title']})), child: Text(tr('Scan now'))),
            OutlinedButton(
              onPressed: () async {
                try {
                  final r = await api.post('/library/sections/$id/credits');
                  say(tr('Looking for credits in {r} videos', {'r': r?['videos']}));
                } catch (e) {
                  say('$e');
                }
              },
              child: Text(tr('Detect credits')),
            ),
            if (type == 'show')
              OutlinedButton(
                onPressed: () async {
                  try {
                    final r = await api.post('/library/sections/$id/intros');
                    say(tr('Listening for intros in {r} episodes', {'r': r?['episodes']}));
                  } catch (e) {
                    say('$e');
                  }
                },
                child: Text(tr('Detect intros')),
              ),
            OutlinedButton(onPressed: () => run(() => api.post('/library/sections/$id/refreshMetadata'), ok: tr('Details for {s} are being reloaded', {'s': s['title']})), child: Text(tr('Reload details'))),
            OutlinedButton(onPressed: () => run(() => api.post('/library/sections/$id/analyze'), ok: tr('The files are being read again')), child: Text(tr('Analyze files'))),
            OutlinedButton(onPressed: () => run(() => api.post('/library/sections/$id/deepanalyze'), ok: tr('Deep analysis runs in the background')), child: Text(tr('Deep analysis'))),
            OutlinedButton(
              onPressed: () async {
                final ok = await showDialog<bool>(
                  context: context,
                  builder: (d) => AlertDialog(
                    content: Text(tr('Remove the titles that are no longer available (files missing) from “{s}” for good?', {'s': s['title']})),
                    actions: [TextButton(onPressed: () => Navigator.pop(d, false), child: Text(tr('Cancel'))), FilledButton(onPressed: () => Navigator.pop(d, true), child: Text(tr('Empty trash')))],
                  ),
                );
                if (ok == true) run(() => api.post('/library/sections/$id/emptyTrash'), ok: tr('Trash emptied'));
              },
              child: Text(tr('Empty trash')),
            ),
            OutlinedButton(onPressed: () => foldersDialog(s), child: Text(tr('Folder'))),
            TextButton(onPressed: () => remove(s), child: Text(tr('Remove'), style: TextStyle(color: context.bad))),
          ]),
        ]),
      ),
    );
  }

  @override
  Widget build(BuildContext context) {
    return Scaffold(
      appBar: AppBar(title: Text(tr('Libraries')), actions: [
        TextButton.icon(onPressed: addDialog, icon: const Icon(Icons.add), label: Text(tr('Add a library'))),
      ]),
      body: error != null
          ? Center(child: Text(error!))
          : !loaded
              ? const Center(child: CircularProgressIndicator())
              : secs.isEmpty
                  ? Center(child: Text(tr('No library yet. Add one above.')))
                  : ListView(padding: const EdgeInsets.all(16), children: [for (final s in secs) card(s)]),
    );
  }
}
