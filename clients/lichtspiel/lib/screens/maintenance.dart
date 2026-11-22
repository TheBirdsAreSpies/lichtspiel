import 'package:flutter/material.dart';
import '../api.dart';
import 'detail.dart' show fmtSize;
import 'downloads.dart' show saveFile;
import 'settings.dart' show fmtDateTime;
import '../widgets/bar.dart';
import '../theme.dart';
import '../i18n.dart';

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
          title: Text(tr('Maintenance')),
          bottom: TabBar(isScrollable: isNarrow(context), tabs: [Tab(text: tr('Backups')), Tab(text: tr('Tools')), Tab(text: tr('Server log')), Tab(text: tr('Reported problems'))]),
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
        actions: [TextButton(onPressed: () => Navigator.pop(d, false), child: Text(tr('Cancel'))), FilledButton(onPressed: () => Navigator.pop(d, true), child: Text(ok))],
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
      if (mounted) _say(context, tr('Backed up: {x}', {'x': x['path']}));
    } catch (e) {
      if (mounted) _say(context, '$e');
    }
    if (mounted) setState(() => busy = false);
    load();
  }

  Future<void> restore(Map<String, dynamic> b) async {
    if (!await _sure(context, tr('Restore this backup? Libraries, users, settings and watch history are then as they were back then. The current state is backed up first.'), tr('Restore')) || !mounted) return;
    try {
      await widget.api.post('/api/maintenance/restore', {'name': b['name']});
      if (mounted) _say(context, tr('Backup restored'));
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
      Text(last != null && last! > 0 ? tr('Last backup: {fmtDateTime}', {'fmtDateTime': fmtDateTime(last)}) : tr('No backup yet.')),
      const SizedBox(height: 12),
      Align(alignment: Alignment.centerLeft, child: FilledButton(onPressed: busy ? null : backupNow, child: Text(tr('Back up now')))),
      const SizedBox(height: 12),
      if (list.isEmpty) Text(tr('There are no backups in the backup folder yet.'), style: TextStyle(color: context.faint)),
      for (final b in list)
        ListTile(
          contentPadding: EdgeInsets.zero,
          title: Text('${b['name']}', style: const TextStyle(fontFamily: 'monospace', fontSize: 13)),
          subtitle: Text('${fmtDateTime(b['time'])} · ${fmtSize((b['size'] ?? 0) as num)}'),
          trailing: TextButton(onPressed: () => restore(b), child: Text(tr('Restore'))),
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
      tool(tr('Shrink the library file'), tr('Compacts the library file. Works while the server is running, too.'), tr('Shrink'), () => run(() async {
            final x = await widget.api.post('/api/maintenance/optimize');
            return tr('Shrunk: {fmtSize} → {fmtSize2}', {'fmtSize': fmtSize((x['before'] ?? 0) as num), 'fmtSize2': fmtSize((x['after'] ?? 0) as num)});
          })),
      tool(tr('Clean up'), tr('Removes leftover conversions and cache files older than a week.'), tr('Clean up'), () => run(() async {
            final x = await widget.api.post('/api/maintenance/cleanup');
            return tr('{fmtSize} freed{x}', {'fmtSize': fmtSize((x['freed'] ?? 0) as num), 'x': x['transcodes_kept'] != null && x['transcodes_kept'] != 0 ? tr(' – running conversions were kept') : ''});
          })),
      tool(tr('Theme music from show intros'), tr('Cuts the theme music of every show still waiting for it now, instead of 3 shows a minute in the maintenance window. One show again: “Cut theme music” in its menu.'), tr('Cut now'), () => run(() async {
            final x = await widget.api.post('/api/maintenance/themes');
            return x['started'] == true ? tr('The theme music of {x} shows is being cut – a message comes when it is done', {'x': x['shows']}) : tr('Every show with a found intro has its theme music already');
          })),
      tool(tr('Report'), tr('Version, system, libraries, settings (without keys) and plugin messages as a text file.'), tr('Save'), () => saveFile(context, widget.api, '/api/maintenance/report')),
      tool(tr('Library file'), tr('A copy of the whole library: titles, watch state, users and settings.'), tr('Save'), () => saveFile(context, widget.api, '/api/maintenance/database')),
      const Divider(height: 32),
      Text(tr('Optimized versions'), style: Theme.of(context).textTheme.titleMedium),
      const SizedBox(height: 8),
      if (error != null) Text(error!) else if (items == null) const Center(child: CircularProgressIndicator()) else ...[
        Text(items.isEmpty ? tr('Nothing optimized yet. Open a title and choose “Optimize”.') : tr('{length} optimized version(s), {fmtSize} in all.', {'length': items.length, 'fmtSize': fmtSize(total)}), style: TextStyle(color: context.faint)),
        for (final i in items.take(30)) ListTile(dense: true, contentPadding: EdgeInsets.zero, title: Text('${i['title']}'), trailing: Text('${i['quality'] ?? ''} · ${fmtSize((i['size'] ?? 0) as num)}')),
        if (items.length > 30) Text(tr('… and {items} more', {'items': items.length - 30}), style: TextStyle(color: context.faint)),
        if (items.isNotEmpty)
          Align(
            alignment: Alignment.centerLeft,
            child: OutlinedButton(
              onPressed: () async {
                if (!await _sure(context, tr('Delete all optimized versions?'), tr('Delete'))) return;
                await run(() async {
                  final x = await widget.api.delete('/api/optimized');
                  return tr('{x} optimized version(s) deleted', {'x': x['removed']});
                });
                load();
              },
              child: Text(tr('Delete all optimized versions')),
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
          ? tr('Nothing has been logged yet.')
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
          OutlinedButton(onPressed: load, child: Text(tr('Refresh'))),
          OutlinedButton(onPressed: () => saveFile(context, widget.api, '/api/log/download'), child: Text(tr('Save as text'))),
          OutlinedButton(onPressed: () => saveFile(context, widget.api, '/api/log/download?gz=1'), child: Text(tr('Save packed (.gz)'))),
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
  static Map<String, String> get kinds => {
    'playback': tr('Does not play / stops'),
    'video': tr('Picture problem'),
    'audio': tr('Sound problem'),
    'subtitles': tr('Subtitle problem'),
    'metadata': tr('Wrong details or poster'),
    'other': tr('Something else'),
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

  String what(Map x) => x['title'] == null ? tr('(removed)') : x['type'] == 'episode' ? '${x['grandparentTitle'] ?? ''} · S${x['parentIndex'] ?? 0}E${x['index'] ?? 0} ${x['title']}' : '${x['title']}';

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
    if (list.isEmpty) return Center(child: Text(tr('No problems reported.')));
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
                TextButton(onPressed: () => run(() => widget.api.put('/api/reports/${p['id']}', {'resolved': p['resolved'] != true})), child: Text(p['resolved'] == true ? tr('Reopen') : tr('Resolved'))),
                IconButton(
                  tooltip: tr('Delete'),
                  icon: const Icon(Icons.delete_outline),
                  onPressed: () async {
                    if (await _sure(context, tr('Delete this report?'), tr('Delete'))) run(() => widget.api.delete('/api/reports/${p['id']}'));
                  },
                ),
              ]),
            ),
          ),
        ),
    ]);
  }
}
