import 'package:flutter/material.dart';
import 'package:flutter/services.dart';
import 'package:shared_preferences/shared_preferences.dart';
import '../api.dart';
import 'users.dart';

String fmtDateTime(dynamic secs) {
  if (secs is! num || secs <= 0) return '–';
  final d = DateTime.fromMillisecondsSinceEpoch((secs * 1000).round());
  String two(int n) => n.toString().padLeft(2, '0');
  return '${two(d.day)}.${two(d.month)}.${d.year} ${two(d.hour)}:${two(d.minute)}';
}

/// The account's settings like the web app's: account, playback, devices and the watchlist feed.
class SettingsScreen extends StatefulWidget {
  final Api api;
  final bool admin;
  const SettingsScreen({super.key, required this.api, required this.admin});
  @override
  State<SettingsScreen> createState() => _SettingsScreenState();
}

class _SettingsScreenState extends State<SettingsScreen> {
  @override
  Widget build(BuildContext context) {
    return DefaultTabController(
      length: 4,
      child: Scaffold(
        appBar: AppBar(
          title: const Text('Einstellungen'),
          actions: [
            if (widget.admin)
              TextButton.icon(
                icon: const Icon(Icons.group_outlined),
                label: const Text('Nutzer & Einladungen'),
                onPressed: () => Navigator.push(context, MaterialPageRoute(builder: (_) => UsersScreen(api: widget.api))),
              ),
          ],
          bottom: const TabBar(tabs: [Tab(text: 'Konto'), Tab(text: 'Wiedergabe'), Tab(text: 'Geräte'), Tab(text: 'Merkliste-Feed')]),
        ),
        body: TabBarView(children: [
          _AccountTab(api: widget.api),
          _PlaybackTab(api: widget.api),
          _DevicesTab(api: widget.api),
          _FeedTab(api: widget.api),
        ]),
      ),
    );
  }
}

Widget _panel(BuildContext context, String title, List<Widget> children) => Card(
      margin: const EdgeInsets.only(bottom: 16),
      child: Padding(
        padding: const EdgeInsets.all(16),
        child: Column(crossAxisAlignment: CrossAxisAlignment.start, children: [
          Text(title, style: Theme.of(context).textTheme.titleMedium),
          const SizedBox(height: 12),
          ...children,
        ]),
      ),
    );

void _say(BuildContext c, String s) => ScaffoldMessenger.of(c).showSnackBar(SnackBar(content: Text(s)));

Future<String?> _askPassword(BuildContext context, String title) {
  final c = TextEditingController();
  return showDialog<String>(
    context: context,
    builder: (d) => AlertDialog(
      title: Text(title),
      content: TextField(controller: c, obscureText: true, autofocus: true, onSubmitted: (v) => Navigator.pop(d, v)),
      actions: [
        TextButton(onPressed: () => Navigator.pop(d), child: const Text('Abbrechen')),
        FilledButton(onPressed: () => Navigator.pop(d, c.text), child: const Text('OK')),
      ],
    ),
  );
}

class _AccountTab extends StatefulWidget {
  final Api api;
  const _AccountTab({required this.api});
  @override
  State<_AccountTab> createState() => _AccountTabState();
}

class _AccountTabState extends State<_AccountTab> {
  Map<String, dynamic>? me;
  final name = TextEditingController();

  @override
  void initState() {
    super.initState();
    widget.api.get('/api/me').then((m) {
      if (mounted && m is Map) {
        setState(() {
          me = Map<String, dynamic>.from(m);
          name.text = '${m['full_name'] ?? ''}';
        });
      }
    }).catchError((_) {});
  }

  Future<void> run(Future<void> Function() f, String ok) async {
    try {
      await f();
      if (mounted) _say(context, ok);
    } catch (e) {
      if (mounted) _say(context, '$e');
    }
  }

  @override
  Widget build(BuildContext context) {
    final m = me;
    if (m == null) return const Center(child: CircularProgressIndicator());
    return ListView(padding: const EdgeInsets.all(16), children: [
      _panel(context, 'Dein Konto', [
        Text('Benutzername: ${m['username']}'),
        const SizedBox(height: 12),
        TextField(controller: name, decoration: const InputDecoration(labelText: 'Voller Name', border: OutlineInputBorder())),
        const SizedBox(height: 12),
        Align(
          alignment: Alignment.centerLeft,
          child: FilledButton(onPressed: () => run(() async => widget.api.post('/api/me/profile', {'full_name': name.text}), 'Gespeichert'), child: const Text('Speichern')),
        ),
      ]),
      _panel(context, 'Zweistufige Anmeldung', [
        Text(m['two_factor'] == true ? 'Die zweistufige Anmeldung ist eingeschaltet.' : 'Ausgeschaltet. Einrichten kannst du sie in der Web-App.'),
      ]),
      _panel(context, 'Passwort', [
        Align(
          alignment: Alignment.centerLeft,
          child: OutlinedButton(
            onPressed: () async {
              final old = await _askPassword(context, 'Aktuelles Passwort');
              if (old == null || old.isEmpty || !context.mounted) return;
              final nw = await _askPassword(context, 'Neues Passwort (mindestens 6 Zeichen)');
              if (nw == null || nw.isEmpty) return;
              run(() async => widget.api.post('/api/me/password', {'old': old, 'new': nw}), 'Passwort geändert');
            },
            child: const Text('Passwort ändern'),
          ),
        ),
      ]),
    ]);
  }
}

class _PlaybackTab extends StatefulWidget {
  final Api api;
  const _PlaybackTab({required this.api});
  @override
  State<_PlaybackTab> createState() => _PlaybackTabState();
}

class _PlaybackTabState extends State<_PlaybackTab> {
  static const qualities = {
    'original': 'Original',
    '2160': '4K (2160p)',
    '1080': '1080p',
    '720': '720p',
    '480': '480p',
    '360': '360p',
  };
  static const langs = {
    '': 'Nicht festgelegt',
    'en': 'Englisch',
    'de': 'Deutsch',
    'fr': 'Französisch',
    'es': 'Spanisch',
    'it': 'Italienisch',
    'pt': 'Portugiesisch',
    'nl': 'Niederländisch',
    'sv': 'Schwedisch',
    'da': 'Dänisch',
    'no': 'Norwegisch',
    'fi': 'Finnisch',
    'pl': 'Polnisch',
    'ru': 'Russisch',
    'tr': 'Türkisch',
    'ja': 'Japanisch',
    'ko': 'Koreanisch',
    'zh': 'Chinesisch',
  };
  static const sizes = [75, 100, 125, 150, 200];

  SharedPreferences? p;

  @override
  void initState() {
    super.initState();
    SharedPreferences.getInstance().then((v) async {
      // The account's values win, like in the web app.
      try {
        final m = await widget.api.get('/api/me');
        final prefs = m is Map ? m['prefs'] : null;
        if (prefs is Map) {
          for (final k in ['quality', 'audioLang', 'subLang']) {
            if (prefs[k] != null) await v.setString(k, '${prefs[k]}');
          }
          if (prefs['subSize'] != null) await v.setInt('subSize', int.tryParse('${prefs['subSize']}') ?? 100);
          if (prefs['autoNext'] != null) await v.setBool('autoNext', '${prefs['autoNext']}' == '1');
        }
      } catch (_) {}
      if (mounted) setState(() => p = v);
    });
  }

  Future<void> save(String k, Object v) async {
    final s = p!;
    if (v is String) await s.setString(k, v);
    if (v is int) await s.setInt(k, v);
    if (v is bool) await s.setBool(k, v);
    setState(() {});
    try {
      await widget.api.post('/api/me/prefs', {k: v is bool ? (v ? '1' : '0') : '$v'});
    } catch (_) {}
  }

  Widget row(String label, Widget control, [String help = '']) => Padding(
        padding: const EdgeInsets.symmetric(vertical: 6),
        child: Row(children: [
          Expanded(
            child: Column(crossAxisAlignment: CrossAxisAlignment.start, children: [
              Text(label),
              if (help.isNotEmpty) Text(help, style: const TextStyle(fontSize: 12, color: Colors.white60)),
            ]),
          ),
          control,
        ]),
      );

  Widget drop(String k, String def, Map<String, String> opts) {
    final cur = p!.getString(k) ?? def;
    return DropdownButton<String>(
      value: opts.containsKey(cur) ? cur : def,
      items: [for (final e in opts.entries) DropdownMenuItem(value: e.key, child: Text(e.value))],
      onChanged: (v) => save(k, v ?? def),
    );
  }

  @override
  Widget build(BuildContext context) {
    if (p == null) return const Center(child: CircularProgressIndicator());
    final s = p!;
    return ListView(padding: const EdgeInsets.all(16), children: [
      _panel(context, 'Qualität', [
        row('Videoqualität', drop('quality', 'original', qualities), 'Die Qualität, mit der ein Video startet. Im Player änderbar.'),
        row('Nächste Folge automatisch', Switch(value: s.getBool('autoNext') ?? true, onChanged: (v) => save('autoNext', v)), 'Nach einer Folge nach 10 Sekunden die nächste starten.'),
      ]),
      _panel(context, 'Ton und Untertitel', [
        row('Bevorzugte Tonsprache', drop('audioLang', '', langs)),
        row('Bevorzugte Untertitelsprache', drop('subLang', '', langs)),
        row(
          'Untertitelgröße',
          DropdownButton<int>(
            value: sizes.contains(s.getInt('subSize')) ? s.getInt('subSize') : 100,
            items: [for (final z in sizes) DropdownMenuItem(value: z, child: Text('$z %'))],
            onChanged: (v) => save('subSize', v ?? 100),
          ),
        ),
      ]),
    ]);
  }
}

class _DevicesTab extends StatefulWidget {
  final Api api;
  const _DevicesTab({required this.api});
  @override
  State<_DevicesTab> createState() => _DevicesTabState();
}

class _DevicesTabState extends State<_DevicesTab> {
  List<Map<String, dynamic>>? tokens;
  String? error;

  @override
  void initState() {
    super.initState();
    load();
  }

  Future<void> load() async {
    try {
      final r = await widget.api.get('/api/tokens');
      final l = ((r?['tokens'] as List?) ?? const []).map((e) => Map<String, dynamic>.from(e)).toList();
      num when(Map<String, dynamic> k) => (k['last_used'] ?? k['created_at'] ?? 0) as num;
      l.sort((a, b) => when(b).compareTo(when(a)));
      if (mounted) setState(() => tokens = l);
    } catch (e) {
      if (mounted) setState(() => error = '$e');
    }
  }

  @override
  Widget build(BuildContext context) {
    if (error != null) return Center(child: Text(error!));
    final l = tokens;
    if (l == null) return const Center(child: CircularProgressIndicator());
    if (l.isEmpty) return const Center(child: Text('Keine Geräte.'));
    return ListView(padding: const EdgeInsets.all(16), children: [
      const Padding(padding: EdgeInsets.only(bottom: 8), child: Text('Browser, Apps und Skripte, die an deinem Konto angemeldet sind.', style: TextStyle(color: Colors.white60))),
      for (final k in l)
        Card(
          child: ListTile(
            leading: const Icon(Icons.devices_other),
            title: Row(children: [
              Flexible(child: Text('${k['last_device'] ?? k['device'] ?? 'Web-App'}')),
              if (k['current'] == true) const Padding(padding: EdgeInsets.only(left: 8), child: Chip(label: Text('dieses'), visualDensity: VisualDensity.compact)),
            ]),
            subtitle: Text('Zuletzt benutzt: ${fmtDateTime(k['last_used'])} · Adresse: ${k['last_ip'] ?? '–'} · Angemeldet: ${fmtDateTime(k['created_at'])}'),
            trailing: k['current'] == true
                ? null
                : TextButton(
                    onPressed: () async {
                      final ok = await showDialog<bool>(
                        context: context,
                        builder: (d) => AlertDialog(
                          content: const Text('Dieses Gerät abmelden? Es muss sich neu anmelden.'),
                          actions: [TextButton(onPressed: () => Navigator.pop(d, false), child: const Text('Abbrechen')), FilledButton(onPressed: () => Navigator.pop(d, true), child: const Text('Abmelden'))],
                        ),
                      );
                      if (ok != true) return;
                      await widget.api.delete('/api/tokens/${k['id']}');
                      load();
                    },
                    child: const Text('Abmelden'),
                  ),
          ),
        ),
    ]);
  }
}

class _FeedTab extends StatefulWidget {
  final Api api;
  const _FeedTab({required this.api});
  @override
  State<_FeedTab> createState() => _FeedTabState();
}

class _FeedTabState extends State<_FeedTab> {
  Map<String, dynamic>? feed;

  @override
  void initState() {
    super.initState();
    widget.api.get('/api/rss').then((r) {
      if (mounted && r is Map) setState(() => feed = Map<String, dynamic>.from(r));
    }).catchError((_) {});
  }

  Future<void> change(Map<String, dynamic> body) async {
    final r = await widget.api.post('/api/rss', body);
    if (mounted && r is Map) setState(() => feed = Map<String, dynamic>.from(r));
  }

  @override
  Widget build(BuildContext context) {
    final f = feed;
    if (f == null) return const Center(child: CircularProgressIndicator());
    final on = f['enabled'] == true;
    return ListView(padding: const EdgeInsets.all(16), children: [
      _panel(context, 'Merkliste als Feed', [
        const Text('Deine Merkliste als RSS-Feed für einen Feedreader oder eine Automatisierung. Wer die Adresse kennt, kann die Liste lesen – halte sie also privat.', style: TextStyle(color: Colors.white60)),
        const SizedBox(height: 12),
        if (on) ...[
          SelectableText('${f['url']}'),
          const SizedBox(height: 12),
          Wrap(spacing: 8, children: [
            OutlinedButton(
              onPressed: () {
                Clipboard.setData(ClipboardData(text: '${f['url']}'));
                _say(context, 'Link kopiert');
              },
              child: const Text('Link kopieren'),
            ),
            OutlinedButton(onPressed: () => change({'regenerate': true}), child: const Text('Neue Adresse')),
            OutlinedButton(onPressed: () => change({'enabled': false}), child: const Text('Ausschalten')),
          ]),
        ] else
          FilledButton(onPressed: () => change({'enabled': true}), child: const Text('Einschalten')),
      ]),
    ]);
  }
}
