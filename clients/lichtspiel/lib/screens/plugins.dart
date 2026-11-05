import 'dart:async';
import 'dart:convert';
import 'package:flutter/material.dart';
import 'package:url_launcher/url_launcher.dart';
import '../api.dart';
import 'settings.dart' show fmtDateTime;

void _say(BuildContext c, String s) => ScaffoldMessenger.of(c).showSnackBar(SnackBar(content: Text(s), duration: const Duration(seconds: 6)));

/// Admin: the installed plugins with their settings, and what they did lately.
class PluginsScreen extends StatefulWidget {
  final Api api;
  const PluginsScreen({super.key, required this.api});
  @override
  State<PluginsScreen> createState() => _PluginsScreenState();
}

class _PluginsScreenState extends State<PluginsScreen> {
  Map<String, dynamic>? data;
  List<Map<String, dynamic>> log = [];
  String? error;
  final settings = <String, TextEditingController>{};

  @override
  void initState() {
    super.initState();
    load();
  }

  @override
  void dispose() {
    for (final c in settings.values) {
      c.dispose();
    }
    super.dispose();
  }

  Future<void> load() async {
    try {
      final r = await widget.api.get('/api/plugins');
      final l = await widget.api.get('/api/plugins/log');
      if (!mounted) return;
      setState(() {
        data = Map<String, dynamic>.from(r);
        log = [for (final x in (l['log'] as List? ?? const [])) Map<String, dynamic>.from(x)];
        for (final p in (r['plugins'] as List? ?? const [])) {
          settings.putIfAbsent('${p['id']}', () => TextEditingController()).text = const JsonEncoder.withIndent('  ').convert(p['settings'] ?? {});
        }
        error = null;
      });
    } catch (e) {
      if (mounted) setState(() => error = '$e');
    }
  }

  Future<void> run(Future<dynamic> Function() f, {String? ok}) async {
    try {
      await f();
      if (ok != null && mounted) _say(context, ok);
    } catch (e) {
      if (mounted) _say(context, '$e');
    }
    load();
  }

  @override
  Widget build(BuildContext context) {
    final d = data;
    final plugins = ((d?['plugins'] as List?) ?? const []).map((e) => Map<String, dynamic>.from(e)).toList();
    return Scaffold(
      appBar: AppBar(title: const Text('Plugins'), actions: [
        TextButton(
          onPressed: () async {
            try {
              final x = await widget.api.post('/api/plugins/reload');
              if (mounted) _say(context, '${x['count']} Plugin(s) gefunden');
            } catch (e) {
              if (mounted) _say(context, '$e');
            }
            load();
          },
          child: const Text('Neu laden'),
        ),
      ]),
      body: error != null
          ? Center(child: Text(error!))
          : d == null
              ? const Center(child: CircularProgressIndicator())
              : ListView(padding: const EdgeInsets.all(16), children: [
                  Text('Plugins bringen Funktionen mit: Details ergänzen, Benachrichtigungen senden, auf die Wiedergabe reagieren. Jedes Plugin liegt in einem eigenen Ordner in „${d['folder']}“ mit einer plugin.json; danach „Neu laden“.', style: const TextStyle(color: Colors.white54)),
                  for (final e in (d['errors'] as List? ?? const [])) Padding(padding: const EdgeInsets.only(top: 8), child: Text('$e', style: const TextStyle(color: Colors.redAccent))),
                  const SizedBox(height: 8),
                  if (plugins.isEmpty) const Padding(padding: EdgeInsets.all(32), child: Center(child: Text('Noch keine Plugins installiert.'))),
                  for (final p in plugins) _plugin(p),
                  const SizedBox(height: 16),
                  Text('Letzte Plugin-Aktivität', style: Theme.of(context).textTheme.titleMedium),
                  if (log.isEmpty) const Text('Noch nichts.', style: TextStyle(color: Colors.white54)),
                  for (final l in log.take(40))
                    ListTile(
                      dense: true,
                      contentPadding: EdgeInsets.zero,
                      leading: Icon(l['ok'] == true ? Icons.check_circle_outline : Icons.error_outline, color: l['ok'] == true ? Colors.greenAccent : Colors.redAccent, size: 18),
                      title: Text('${l['plugin']} · ${l['event']}'),
                      subtitle: Text('${fmtDateTime(l['at'])}${'${l['detail'] ?? ''}'.isEmpty ? '' : ' · ${l['detail']}'}'),
                    ),
                  const SizedBox(height: 16),
                  Text('Ereignisse, auf die ein Plugin hören kann', style: Theme.of(context).textTheme.titleMedium),
                  Wrap(spacing: 8, children: [for (final e in (d['events'] as List? ?? const [])) Chip(label: Text('$e', style: const TextStyle(fontFamily: 'monospace', fontSize: 12)), visualDensity: VisualDensity.compact)]),
                ]),
    );
  }

  Widget _plugin(Map<String, dynamic> p) {
    final on = p['enabled'] == true;
    final id = '${p['id']}';
    return Card(
      margin: const EdgeInsets.only(bottom: 12),
      child: Padding(
        padding: const EdgeInsets.all(14),
        child: Column(crossAxisAlignment: CrossAxisAlignment.start, children: [
          Row(children: [
            Expanded(child: Text('${p['name']}  v${p['version'] ?? '?'}${p['author'] != null ? '  von ${p['author']}' : ''}', style: const TextStyle(fontWeight: FontWeight.w600, fontSize: 16))),
            Text(on ? 'an' : 'aus', style: TextStyle(color: on ? Colors.greenAccent : Colors.white54)),
            const SizedBox(width: 8),
            on
                ? OutlinedButton(onPressed: () => run(() => widget.api.post('/api/plugins/$id/disable')), child: const Text('Ausschalten'))
                : FilledButton(onPressed: () => run(() => widget.api.post('/api/plugins/$id/enable')), child: const Text('Einschalten')),
          ]),
          if ('${p['description'] ?? ''}'.isNotEmpty) Padding(padding: const EdgeInsets.only(top: 6), child: Text('${p['description']}')),
          Padding(
            padding: const EdgeInsets.only(top: 6),
            child: Wrap(spacing: 6, runSpacing: 4, children: [for (final h in (p['hooks'] as List? ?? const [])) Chip(label: Text('$h', style: const TextStyle(fontSize: 11)), visualDensity: VisualDensity.compact)]),
          ),
          const SizedBox(height: 8),
          TextField(controller: settings[id], minLines: 3, maxLines: 10, style: const TextStyle(fontFamily: 'monospace', fontSize: 12), decoration: const InputDecoration(labelText: 'Einstellungen (JSON)', border: OutlineInputBorder())),
          const SizedBox(height: 8),
          Align(
            alignment: Alignment.centerLeft,
            child: OutlinedButton(
              onPressed: () {
                Object? v;
                try {
                  v = jsonDecode(settings[id]!.text);
                } catch (e) {
                  _say(context, 'Kein gültiges JSON: $e');
                  return;
                }
                run(() => widget.api.put('/api/plugins/$id/settings', v), ok: 'Gespeichert');
              },
              child: const Text('Einstellungen speichern'),
            ),
          ),
        ]),
      ),
    );
  }
}

/// Plugins that give each person a panel of their own (e.g. Connect Trakt). The plugin answers "panel" with
/// {title, text, link, buttons: [{label, action, primary, confirm}]}; a button's answer may ask again (poll).
class PluginPanels extends StatefulWidget {
  final Api api;
  const PluginPanels({super.key, required this.api});
  @override
  State<PluginPanels> createState() => _PluginPanelsState();
}

class _PluginPanelsState extends State<PluginPanels> {
  List<Map<String, dynamic>> panels = [];

  @override
  void initState() {
    super.initState();
    widget.api.get('/api/plugins/panels').then((r) {
      if (mounted && r is Map) setState(() => panels = [for (final p in (r['panels'] as List? ?? const [])) Map<String, dynamic>.from(p)]);
    }).catchError((_) {});
  }

  @override
  Widget build(BuildContext context) => Column(children: [for (final p in panels) _PluginPanel(key: ValueKey(p['id']), api: widget.api, id: '${p['id']}', name: '${p['name']}')]);
}

class _PluginPanel extends StatefulWidget {
  final Api api;
  final String id, name;
  const _PluginPanel({super.key, required this.api, required this.id, required this.name});
  @override
  State<_PluginPanel> createState() => _PluginPanelState();
}

class _PluginPanelState extends State<_PluginPanel> {
  Map<String, dynamic>? panel;

  @override
  void initState() {
    super.initState();
    load();
  }

  Future<dynamic> call(String action) => widget.api.post('/api/plugins/${widget.id}/call/$action', {'lang': 'de'});

  Future<void> load() async {
    Map<String, dynamic> p;
    try {
      p = Map<String, dynamic>.from(await call('panel'));
    } catch (e) {
      p = {'title': widget.name, 'text': '$e', 'buttons': []};
    }
    if (mounted) setState(() => panel = p);
  }

  Future<void> press(Map b) async {
    if (b['confirm'] != null) {
      final ok = await showDialog<bool>(
        context: context,
        builder: (d) => AlertDialog(
          content: Text('${b['confirm']}'),
          actions: [TextButton(onPressed: () => Navigator.pop(d, false), child: const Text('Abbrechen')), FilledButton(onPressed: () => Navigator.pop(d, true), child: const Text('OK'))],
        ),
      );
      if (ok != true) return;
    }
    dynamic r;
    try {
      r = await call('${b['action']}');
    } catch (e) {
      if (mounted) _say(context, '$e');
      return;
    }
    if (!mounted) return;
    if (r is Map && r['poll'] != null) {
      await showDialog(context: context, barrierDismissible: false, builder: (_) => _PollDialog(api: widget.api, id: widget.id, title: '${b['label']}', first: Map<String, dynamic>.from(r)));
    } else if (r is Map) {
      final counts = r['marked'] != null ? ' ${r['matched']} in deinen Bibliotheken gefunden, ${r['marked']} neu als gesehen markiert.' : '';
      if (r['error'] != null) {
        _say(context, '${r['error']}');
      } else if (r['message'] != null || counts.isNotEmpty) {
        _say(context, '${r['message'] ?? ''}$counts');
      }
    }
    load();
  }

  @override
  Widget build(BuildContext context) {
    final p = panel;
    return Card(
      margin: const EdgeInsets.only(bottom: 16),
      child: Padding(
        padding: const EdgeInsets.all(16),
        child: p == null
            ? const Text('Lädt…')
            : Column(crossAxisAlignment: CrossAxisAlignment.start, children: [
                Row(children: [
                  Text('${p['title'] ?? widget.name}', style: Theme.of(context).textTheme.titleMedium),
                  if (p['status'] == 'on') const Padding(padding: EdgeInsets.only(left: 10), child: Text('verbunden', style: TextStyle(color: Colors.greenAccent))),
                ]),
                if (p['text'] != null) Padding(padding: const EdgeInsets.only(top: 8), child: Text('${p['text']}')),
                if (p['link'] != null)
                  TextButton(onPressed: () => launchUrl(Uri.parse('${p['link']}'), mode: LaunchMode.externalApplication), child: Text('${p['link']}')),
                if ((p['buttons'] as List? ?? const []).isNotEmpty)
                  Padding(
                    padding: const EdgeInsets.only(top: 12),
                    child: Wrap(spacing: 8, runSpacing: 8, children: [
                      for (final b in (p['buttons'] as List))
                        b['primary'] == true ? FilledButton(onPressed: () => press(b), child: Text('${b['label']}')) : OutlinedButton(onPressed: () => press(b), child: Text('${b['label']}')),
                    ]),
                  ),
              ]),
      ),
    );
  }
}

/// A code to type in somewhere else (a device login): asks the plugin again until it is done.
class _PollDialog extends StatefulWidget {
  final Api api;
  final String id, title;
  final Map<String, dynamic> first;
  const _PollDialog({required this.api, required this.id, required this.title, required this.first});
  @override
  State<_PollDialog> createState() => _PollDialogState();
}

class _PollDialogState extends State<_PollDialog> {
  String message = '', code = '', link = '';
  Timer? timer;
  bool stopped = false;

  @override
  void initState() {
    super.initState();
    next(widget.first);
  }

  @override
  void dispose() {
    stopped = true;
    timer?.cancel();
    super.dispose();
  }

  void next(Map r) {
    if (stopped || !mounted) return;
    setState(() {
      if (r['message'] != null) message = '${r['message']}';
      if (r['code'] != null) code = '${r['code']}';
      if (r['link'] != null) link = '${r['link']}';
    });
    final poll = r['poll'];
    if (poll == null) {
      Navigator.pop(context);
      if (r['error'] != null) {
        _say(context, '${r['error']}');
      } else if (r['message'] != null) {
        _say(context, '${r['message']}');
      }
      return;
    }
    final secs = ((poll['seconds'] ?? 5) as num).toInt().clamp(2, 60);
    timer = Timer(Duration(seconds: secs), () async {
      try {
        final n = await widget.api.post('/api/plugins/${widget.id}/call/${poll['action']}', {'lang': 'de'});
        next(Map<String, dynamic>.from(n));
      } catch (e) {
        if (mounted) {
          Navigator.pop(context);
          _say(context, '$e');
        }
      }
    });
  }

  @override
  Widget build(BuildContext context) {
    return AlertDialog(
      title: Text(widget.title),
      content: Column(mainAxisSize: MainAxisSize.min, crossAxisAlignment: CrossAxisAlignment.start, children: [
        if (message.isNotEmpty) Text(message),
        if (code.isNotEmpty) Padding(padding: const EdgeInsets.symmetric(vertical: 12), child: SelectableText(code, style: const TextStyle(fontSize: 32, fontWeight: FontWeight.w700, letterSpacing: 4))),
        if (link.isNotEmpty) TextButton(onPressed: () => launchUrl(Uri.parse(link), mode: LaunchMode.externalApplication), child: Text(link)),
        const Text('Wartet, bis der Code eingegeben wurde…', style: TextStyle(color: Colors.white54)),
      ]),
      actions: [TextButton(onPressed: () => Navigator.pop(context), child: const Text('Abbrechen'))],
    );
  }
}
