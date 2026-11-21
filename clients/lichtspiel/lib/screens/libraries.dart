import 'dart:async';
import 'package:flutter/material.dart';
import '../api.dart';
import '../theme.dart';

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

  static const kinds = {'movie': 'Filme', 'show': 'Serien', 'music': 'Musik', 'other': 'Andere Videos'};
  static const intervals = {
    '': 'Wie der Server',
    '0': 'Nie von selbst',
    '15': 'alle 15 Minuten',
    '30': 'alle 30 Minuten',
    '60': 'stündlich',
    '120': 'alle 2 Stunden',
    '360': 'alle 6 Stunden',
    '720': 'alle 12 Stunden',
    '1440': 'täglich',
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
    if (s is! num || s <= 0) return 'nie';
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
          title: const Text('Bibliothek hinzufügen'),
          content: SizedBox(
            width: 460,
            child: SingleChildScrollView(
              child: Column(mainAxisSize: MainAxisSize.min, crossAxisAlignment: CrossAxisAlignment.start, children: [
                TextField(controller: name, decoration: const InputDecoration(labelText: 'Name', hintText: 'Filme')),
                const SizedBox(height: 12),
                DropdownButtonFormField<String>(
                  isExpanded: true,
                  initialValue: kind,
                  decoration: const InputDecoration(labelText: 'Art'),
                  items: [for (final e in kinds.entries) DropdownMenuItem(value: e.key, child: Text(e.value))],
                  onChanged: (v) => set(() => kind = v ?? 'movie'),
                ),
                const SizedBox(height: 12),
                DropdownButtonFormField<String>(
                  isExpanded: true,
                  initialValue: lang,
                  decoration: const InputDecoration(labelText: 'Sprache der Titel und Handlungen'),
                  items: [DropdownMenuItem(value: '', child: Text('Wie der Server ($serverLang)')), for (final l in langs) DropdownMenuItem(value: '${l['code']}', child: Text('${l['name']}'))],
                  onChanged: (v) => set(() => lang = v ?? ''),
                ),
                const SizedBox(height: 12),
                TextField(
                  controller: paths,
                  minLines: 3,
                  maxLines: 6,
                  decoration: const InputDecoration(labelText: 'Ordner', hintText: 'Ein Ordner pro Zeile, z. B. D:\\Filme oder /volume1/video/Filme', border: OutlineInputBorder()),
                ),
              ]),
            ),
          ),
          actions: [TextButton(onPressed: () => Navigator.pop(d, false), child: const Text('Abbrechen')), FilledButton(onPressed: () => Navigator.pop(d, true), child: const Text('Hinzufügen'))],
        ),
      ),
    );
    if (ok != true) return;
    run(() => api.post('/library/sections', {'name': name.text, 'type': kind, 'paths': paths.text, 'language': lang}), ok: 'Bibliothek hinzugefügt, wird gescannt');
  }

  Future<void> foldersDialog(Map<String, dynamic> s) async {
    final c = TextEditingController(text: ((s['paths'] as List?) ?? const []).join('\n'));
    final ok = await showDialog<bool>(
      context: context,
      builder: (d) => AlertDialog(
        title: Text('Ordner von ${s['title']}'),
        content: SizedBox(width: 460, child: TextField(controller: c, minLines: 3, maxLines: 8, decoration: const InputDecoration(helperText: 'Ein Ordner pro Zeile', border: OutlineInputBorder()))),
        actions: [TextButton(onPressed: () => Navigator.pop(d, false), child: const Text('Abbrechen')), FilledButton(onPressed: () => Navigator.pop(d, true), child: const Text('Speichern'))],
      ),
    );
    if (ok != true) return;
    run(() => api.put('/library/sections/${s['key']}', {'paths': c.text.split('\n').map((e) => e.trim()).where((e) => e.isNotEmpty).toList()}), ok: 'Gespeichert');
  }

  Future<void> remove(Map<String, dynamic> s) async {
    final c = TextEditingController();
    final ok = await showDialog<bool>(
      context: context,
      builder: (d) => AlertDialog(
        title: Text('Bibliothek „${s['title']}“ entfernen?'),
        content: Column(mainAxisSize: MainAxisSize.min, crossAxisAlignment: CrossAxisAlignment.start, children: [
          const Text('Deine Dateien bleiben auf der Platte. Tippe zur Bestätigung den Namen ein:'),
          TextField(controller: c, autofocus: true),
        ]),
        actions: [
          TextButton(onPressed: () => Navigator.pop(d, false), child: const Text('Abbrechen')),
          FilledButton(onPressed: () => Navigator.pop(d, c.text.trim() == '${s['title']}'), child: const Text('Entfernen')),
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
            if (s['scanning'] == true) Padding(padding: EdgeInsets.only(left: 8), child: Text('scannt…', style: TextStyle(color: context.gold))),
          ]),
          Text('${type == 'show' ? '${s['count']} Serien, ' : ''}${s['videoCount'] ?? 0} Videos · letzter Scan: ${when(s['scannedAt'])}', style: TextStyle(color: context.dim)),
          Text(((s['paths'] as List?) ?? const []).join(' · '), style: TextStyle(fontSize: 12, color: context.ghost)),
          Wrap(spacing: 20, crossAxisAlignment: WrapCrossAlignment.center, children: [
            Row(mainAxisSize: MainAxisSize.min, children: [
              Checkbox(value: s['creditsDetection'] != false, onChanged: (v) => run(() => api.put('/library/sections/$id', {'credits_detection': v}), ok: 'Gespeichert')),
              const Text('Abspann erkennen'),
            ]),
            if (type == 'show')
              Row(mainAxisSize: MainAxisSize.min, children: [
                Checkbox(value: s['introDetection'] != false, onChanged: (v) => run(() => api.put('/library/sections/$id', {'intro_detection': v}), ok: 'Gespeichert')),
                const Text('Intros erkennen'),
              ]),
            Row(mainAxisSize: MainAxisSize.min, children: [
              const Text('Scan-Intervall  '),
              DropdownButton<String>(
                value: intervals.containsKey(interval) ? interval : '',
                items: [for (final e in intervals.entries) DropdownMenuItem(value: e.key, child: Text(e.value))],
                onChanged: (v) => run(() => api.put('/library/sections/$id', {'scan_interval_minutes': (v ?? '').isEmpty ? null : int.parse(v!)}), ok: 'Gespeichert'),
              ),
            ]),
            Row(mainAxisSize: MainAxisSize.min, children: [
              const Text('Sprache der Details  '),
              DropdownButton<String>(
                value: langs.any((l) => l['code'] == s['language']) ? '${s['language']}' : '',
                items: [DropdownMenuItem(value: '', child: Text('Wie der Server ($serverLang)')), for (final l in langs) DropdownMenuItem(value: '${l['code']}', child: Text('${l['name']}'))],
                onChanged: (v) async {
                  try {
                    final r = await api.put('/library/sections/$id', {'language': v ?? ''});
                    say(r is Map && r['refetching'] != null && r['refetching'] != 0 ? 'Details für ${r['refetching']} Titel werden neu geladen' : 'Gespeichert');
                    load();
                  } catch (e) {
                    say('$e');
                  }
                },
              ),
            ]),
          ]),
          Wrap(spacing: 8, runSpacing: 4, children: [
            OutlinedButton(onPressed: () => run(() => api.post('/library/sections/$id/refresh'), ok: 'Scanne ${s['title']}'), child: const Text('Jetzt scannen')),
            OutlinedButton(
              onPressed: () async {
                try {
                  final r = await api.post('/library/sections/$id/credits');
                  say('Prüfe ${r?['videos']} Videos auf Abspann');
                } catch (e) {
                  say('$e');
                }
              },
              child: const Text('Abspann erkennen'),
            ),
            if (type == 'show')
              OutlinedButton(
                onPressed: () async {
                  try {
                    final r = await api.post('/library/sections/$id/intros');
                    say('Höre in ${r?['episodes']} Folgen nach Intros');
                  } catch (e) {
                    say('$e');
                  }
                },
                child: const Text('Intros erkennen'),
              ),
            OutlinedButton(onPressed: () => run(() => api.post('/library/sections/$id/refreshMetadata'), ok: 'Details für ${s['title']} werden neu geladen'), child: const Text('Details neu laden')),
            OutlinedButton(onPressed: () => run(() => api.post('/library/sections/$id/analyze'), ok: 'Dateien werden neu gelesen'), child: const Text('Dateien analysieren')),
            OutlinedButton(onPressed: () => run(() => api.post('/library/sections/$id/deepanalyze'), ok: 'Tiefenanalyse läuft im Hintergrund'), child: const Text('Tiefenanalyse')),
            OutlinedButton(
              onPressed: () async {
                final ok = await showDialog<bool>(
                  context: context,
                  builder: (d) => AlertDialog(
                    content: Text('Nicht mehr verfügbare Titel (Dateien fehlen) aus „${s['title']}“ endgültig entfernen?'),
                    actions: [TextButton(onPressed: () => Navigator.pop(d, false), child: const Text('Abbrechen')), FilledButton(onPressed: () => Navigator.pop(d, true), child: const Text('Papierkorb leeren'))],
                  ),
                );
                if (ok == true) run(() => api.post('/library/sections/$id/emptyTrash'), ok: 'Papierkorb geleert');
              },
              child: const Text('Papierkorb leeren'),
            ),
            OutlinedButton(onPressed: () => foldersDialog(s), child: const Text('Ordner')),
            TextButton(onPressed: () => remove(s), child: Text('Entfernen', style: TextStyle(color: context.bad))),
          ]),
        ]),
      ),
    );
  }

  @override
  Widget build(BuildContext context) {
    return Scaffold(
      appBar: AppBar(title: const Text('Bibliotheken'), actions: [
        TextButton.icon(onPressed: addDialog, icon: const Icon(Icons.add), label: const Text('Bibliothek hinzufügen')),
      ]),
      body: error != null
          ? Center(child: Text(error!))
          : !loaded
              ? const Center(child: CircularProgressIndicator())
              : secs.isEmpty
                  ? const Center(child: Text('Noch keine Bibliothek. Füge oben eine hinzu.'))
                  : ListView(padding: const EdgeInsets.all(16), children: [for (final s in secs) card(s)]),
    );
  }
}
