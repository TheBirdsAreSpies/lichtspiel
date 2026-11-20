import 'package:flutter/material.dart';
import '../api.dart';
import 'detail.dart' show fmtSize;
import 'downloads.dart' show saveFile;
import 'settings.dart' show fmtDateTime;
import '../widgets/bar.dart';
import '../theme.dart';

/// Admin: backups, compacting and cleaning up, optimized versions, the server log and reported problems.
class MaintenanceScreen extends StatelessWidget {
  final Api api;
  const MaintenanceScreen({super.key, required this.api});

  @override
  Widget build(BuildContext context) {
    return DefaultTabController(
      length: 4,
      child: Scaffold(
        appBar: AppBar(
          title: const Text('Wartung'),
          bottom: TabBar(isScrollable: isNarrow(context), tabs: const [Tab(text: 'Sicherungen'), Tab(text: 'Werkzeuge'), Tab(text: 'Server-Log'), Tab(text: 'Gemeldete Probleme')]),
        ),
        body: TabBarView(children: [
          _BackupsTab(api: api),
          _ToolsTab(api: api),
          _LogTab(api: api),
          _ReportsTab(api: api),
        ]),
      ),
    );
  }
}

void _say(BuildContext c, String s) => ScaffoldMessenger.of(c).showSnackBar(SnackBar(content: Text(s)));

Future<bool> _sure(BuildContext context, String text, String ok) async =>
    await showDialog<bool>(
      context: context,
      builder: (d) => AlertDialog(
        content: Text(text),
        actions: [TextButton(onPressed: () => Navigator.pop(d, false), child: const Text('Abbrechen')), FilledButton(onPressed: () => Navigator.pop(d, true), child: Text(ok))],
      ),
    ) ==
    true;

class _BackupsTab extends StatefulWidget {
  final Api api;
  const _BackupsTab({required this.api});
  @override
  State<_BackupsTab> createState() => _BackupsTabState();
}

class _BackupsTabState extends State<_BackupsTab> {
  List<Map<String, dynamic>>? backups;
  num? last;
  String? error;
  bool busy = false;

  @override
  void initState() {
    super.initState();
    load();
  }

  Future<void> load() async {
    try {
      final st = await widget.api.get('/api/settings');
      final r = await widget.api.get('/api/maintenance/backups');
      if (!mounted) return;
      setState(() {
        last = st['settings']?['maintenance_last']?['backup'] as num?;
        backups = [for (final b in (r['backups'] as List? ?? const [])) Map<String, dynamic>.from(b)];
        error = null;
      });
    } catch (e) {
      if (mounted) setState(() => error = '$e');
    }
  }

  Future<void> backupNow() async {
    setState(() => busy = true);
    try {
      final x = await widget.api.post('/api/maintenance/backup');
      if (mounted) _say(context, 'Gesichert: ${x['path']}');
    } catch (e) {
      if (mounted) _say(context, '$e');
    }
    if (mounted) setState(() => busy = false);
    load();
  }

  Future<void> restore(Map<String, dynamic> b) async {
    if (!await _sure(context, 'Diese Sicherung zurückspielen? Bibliotheken, Nutzer, Einstellungen und Seherverlauf sind danach wieder wie damals. Der jetzige Stand wird vorher gesichert.', 'Zurückspielen') || !mounted) return;
    try {
      await widget.api.post('/api/maintenance/restore', {'name': b['name']});
      if (mounted) _say(context, 'Sicherung zurückgespielt');
    } catch (e) {
      if (mounted) _say(context, '$e');
    }
    load();
  }

  @override
  Widget build(BuildContext context) {
    if (error != null) return Center(child: Text(error!));
    final list = backups;
    if (list == null) return const Center(child: CircularProgressIndicator());
    return ListView(padding: const EdgeInsets.all(16), children: [
      Text(last != null && last! > 0 ? 'Letzte Sicherung: ${fmtDateTime(last)}' : 'Noch keine Sicherung.'),
      const SizedBox(height: 12),
      Align(alignment: Alignment.centerLeft, child: FilledButton(onPressed: busy ? null : backupNow, child: const Text('Jetzt sichern'))),
      const SizedBox(height: 12),
      if (list.isEmpty) Text('Im Sicherungsordner liegen noch keine Sicherungen.', style: TextStyle(color: context.faint)),
      for (final b in list)
        ListTile(
          contentPadding: EdgeInsets.zero,
          title: Text('${b['name']}', style: const TextStyle(fontFamily: 'monospace', fontSize: 13)),
          subtitle: Text('${fmtDateTime(b['time'])} · ${fmtSize((b['size'] ?? 0) as num)}'),
          trailing: TextButton(onPressed: () => restore(b), child: const Text('Zurückspielen')),
        ),
    ]);
  }
}

class _ToolsTab extends StatefulWidget {
  final Api api;
  const _ToolsTab({required this.api});
  @override
  State<_ToolsTab> createState() => _ToolsTabState();
}

class _ToolsTabState extends State<_ToolsTab> {
  List<Map<String, dynamic>>? optimized;
  String? error;

  @override
  void initState() {
    super.initState();
    load();
  }

  Future<void> load() async {
    try {
      final r = await widget.api.get('/api/optimized');
      if (mounted) setState(() => optimized = [for (final i in (r['items'] as List? ?? const [])) Map<String, dynamic>.from(i)]);
    } catch (e) {
      if (mounted) setState(() => error = '$e');
    }
  }

  Future<void> run(Future<String> Function() f) async {
    try {
      final m = await f();
      if (mounted) _say(context, m);
    } catch (e) {
      if (mounted) _say(context, '$e');
    }
  }

  Widget tool(String title, String help, String button, VoidCallback onPressed) => ListTile(
        contentPadding: EdgeInsets.zero,
        title: Text(title),
        subtitle: Text(help, style: TextStyle(color: context.faint)),
        trailing: OutlinedButton(onPressed: onPressed, child: Text(button)),
      );

  @override
  Widget build(BuildContext context) {
    final items = optimized;
    final total = items == null ? 0 : items.fold<num>(0, (a, i) => a + ((i['size'] ?? 0) as num));
    return ListView(padding: const EdgeInsets.all(16), children: [
      tool('Bibliotheksdatei verkleinern', 'Verdichtet die Bibliotheksdatei. Geht auch, während der Server läuft.', 'Verkleinern', () => run(() async {
            final x = await widget.api.post('/api/maintenance/optimize');
            return 'Verkleinert: ${fmtSize((x['before'] ?? 0) as num)} → ${fmtSize((x['after'] ?? 0) as num)}';
          })),
      tool('Aufräumen', 'Entfernt übrig gebliebene Umwandlungen und Cache-Dateien, die älter als eine Woche sind.', 'Aufräumen', () => run(() async {
            final x = await widget.api.post('/api/maintenance/cleanup');
            return '${fmtSize((x['freed'] ?? 0) as num)} frei${x['transcodes_kept'] != null && x['transcodes_kept'] != 0 ? ' – laufende Umwandlungen blieben' : ''}';
          })),
      tool('Bericht', 'Version, System, Bibliotheken, Einstellungen (ohne Schlüssel) und Plugin-Meldungen als Textdatei.', 'Speichern', () => saveFile(context, widget.api, '/api/maintenance/report')),
      tool('Bibliotheksdatei', 'Eine Kopie der ganzen Bibliothek: Titel, Seherstand, Nutzer und Einstellungen.', 'Speichern', () => saveFile(context, widget.api, '/api/maintenance/database')),
      const Divider(height: 32),
      Text('Optimierte Versionen', style: Theme.of(context).textTheme.titleMedium),
      const SizedBox(height: 8),
      if (error != null) Text(error!) else if (items == null) const Center(child: CircularProgressIndicator()) else ...[
        Text(items.isEmpty ? 'Noch nichts optimiert. Öffne einen Titel und wähle „Optimieren“.' : '${items.length} optimierte Version(en), ${fmtSize(total)} insgesamt.', style: TextStyle(color: context.faint)),
        for (final i in items.take(30)) ListTile(dense: true, contentPadding: EdgeInsets.zero, title: Text('${i['title']}'), trailing: Text('${i['quality'] ?? ''} · ${fmtSize((i['size'] ?? 0) as num)}')),
        if (items.length > 30) Text('… und ${items.length - 30} weitere', style: TextStyle(color: context.faint)),
        if (items.isNotEmpty)
          Align(
            alignment: Alignment.centerLeft,
            child: OutlinedButton(
              onPressed: () async {
                if (!await _sure(context, 'Alle optimierten Versionen löschen?', 'Löschen')) return;
                await run(() async {
                  final x = await widget.api.delete('/api/optimized');
                  return '${x['removed']} optimierte Version(en) gelöscht';
                });
                load();
              },
              child: const Text('Alle optimierten Versionen löschen'),
            ),
          ),
      ],
    ]);
  }
}

class _LogTab extends StatefulWidget {
  final Api api;
  const _LogTab({required this.api});
  @override
  State<_LogTab> createState() => _LogTabState();
}

class _LogTabState extends State<_LogTab> {
  String? text;

  @override
  void initState() {
    super.initState();
    load();
  }

  Future<void> load() async {
    try {
      final r = await widget.api.get('/api/log');
      final lines = (r['lines'] as List? ?? const []);
      if (!mounted) return;
      setState(() => text = lines.isEmpty
          ? 'Es wurde noch nichts protokolliert.'
          : lines.skip(lines.length > 200 ? lines.length - 200 : 0).map((l) => '${fmtDateTime(l['t'])}  ${l['msg']}').join('\n'));
    } catch (e) {
      if (mounted) setState(() => text = '$e');
    }
  }

  @override
  Widget build(BuildContext context) {
    return Column(children: [
      Padding(
        padding: const EdgeInsets.all(12),
        child: Wrap(spacing: 8, children: [
          OutlinedButton(onPressed: load, child: const Text('Aktualisieren')),
          OutlinedButton(onPressed: () => saveFile(context, widget.api, '/api/log/download'), child: const Text('Als Text speichern')),
          OutlinedButton(onPressed: () => saveFile(context, widget.api, '/api/log/download?gz=1'), child: const Text('Gepackt speichern (.gz)')),
        ]),
      ),
      Expanded(
        child: text == null
            ? const Center(child: CircularProgressIndicator())
            : SingleChildScrollView(padding: const EdgeInsets.all(12), reverse: true, child: SelectableText(text!, style: const TextStyle(fontFamily: 'monospace', fontSize: 12))),
      ),
    ]);
  }
}

class _ReportsTab extends StatefulWidget {
  final Api api;
  const _ReportsTab({required this.api});
  @override
  State<_ReportsTab> createState() => _ReportsTabState();
}

class _ReportsTabState extends State<_ReportsTab> {
  static const kinds = {
    'playback': 'Spielt nicht ab / bricht ab',
    'video': 'Bildproblem',
    'audio': 'Tonproblem',
    'subtitles': 'Untertitelproblem',
    'metadata': 'Falsche Details oder falsches Poster',
    'other': 'Etwas anderes',
  };
  List<Map<String, dynamic>>? reports;
  String? error;

  @override
  void initState() {
    super.initState();
    load();
  }

  Future<void> load() async {
    try {
      final r = await widget.api.get('/api/reports');
      if (mounted) setState(() => reports = [for (final x in (r['reports'] as List? ?? const [])) Map<String, dynamic>.from(x)]);
    } catch (e) {
      if (mounted) setState(() => error = '$e');
    }
  }

  String what(Map x) => x['title'] == null ? '(entfernt)' : x['type'] == 'episode' ? '${x['grandparentTitle'] ?? ''} · S${x['parentIndex'] ?? 0}E${x['index'] ?? 0} ${x['title']}' : '${x['title']}';

  Future<void> run(Future<dynamic> Function() f) async {
    try {
      await f();
    } catch (e) {
      if (mounted) _say(context, '$e');
    }
    load();
  }

  @override
  Widget build(BuildContext context) {
    if (error != null) return Center(child: Text(error!));
    final list = reports;
    if (list == null) return const Center(child: CircularProgressIndicator());
    if (list.isEmpty) return const Center(child: Text('Keine Probleme gemeldet.'));
    return ListView(padding: const EdgeInsets.all(16), children: [
      for (final p in list)
        Opacity(
          opacity: p['resolved'] == true ? 0.55 : 1,
          child: Card(
            child: ListTile(
              title: Text(what(p['item'] as Map? ?? const {}), maxLines: 2, overflow: TextOverflow.ellipsis),
              subtitle: Text([kinds['${p['kind']}'] ?? '${p['kind']}', if ('${p['note'] ?? ''}'.isNotEmpty) '${p['note']}', '${p['user'] ?? '–'} · ${fmtDateTime(p['at'])}'].join('\n')),
              isThreeLine: true,
              trailing: Row(mainAxisSize: MainAxisSize.min, children: [
                TextButton(onPressed: () => run(() => widget.api.put('/api/reports/${p['id']}', {'resolved': p['resolved'] != true})), child: Text(p['resolved'] == true ? 'Wieder öffnen' : 'Erledigt')),
                IconButton(
                  tooltip: 'Löschen',
                  icon: const Icon(Icons.delete_outline),
                  onPressed: () async {
                    if (await _sure(context, 'Diese Meldung löschen?', 'Löschen')) run(() => widget.api.delete('/api/reports/${p['id']}'));
                  },
                ),
              ]),
            ),
          ),
        ),
    ]);
  }
}
