import 'package:flutter/material.dart';
import 'package:flutter/services.dart';
import 'package:shared_preferences/shared_preferences.dart';
import 'package:url_launcher/url_launcher.dart';
import '../api.dart';
import 'libraries.dart';
import 'maintenance.dart';
import 'player.dart' show playerQualities, playerSubSizes, qualityKey, subSizeKey;
import 'plugins.dart';
import 'server_settings.dart';
import 'users.dart';
import '../widgets/bar.dart';
import '../theme.dart';
import '../widgets/states.dart';
import '../i18n.dart';

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
          title: Text(tr('Settings')),
          actions: [
            if (widget.admin)
              ...barActions(context, labels: true, [
                BarAction(Icons.video_library_outlined, tr('Libraries'), () => Navigator.push(context, MaterialPageRoute(builder: (_) => LibrariesScreen(api: widget.api)))),
                BarAction(Icons.dns_outlined, tr('Server'), () => Navigator.push(context, MaterialPageRoute(builder: (_) => ServerSettingsScreen(api: widget.api)))),
                BarAction(Icons.extension_outlined, tr('Plugins'), () => Navigator.push(context, MaterialPageRoute(builder: (_) => PluginsScreen(api: widget.api)))),
                BarAction(Icons.build_outlined, tr('Maintenance'), () => Navigator.push(context, MaterialPageRoute(builder: (_) => MaintenanceScreen(api: widget.api)))),
                BarAction(Icons.group_outlined, tr('Users & invites'), () => Navigator.push(context, MaterialPageRoute(builder: (_) => UsersScreen(api: widget.api)))),
              ]),
          ],
          bottom: TabBar(isScrollable: isNarrow(context), tabs: [Tab(text: tr('Account')), Tab(text: tr('Playback')), Tab(text: tr('Devices')), Tab(text: tr('Watchlist feed'))]),
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

Future<String?> _askPassword(BuildContext context, String title, {bool secret = true}) {
  final c = TextEditingController();
  return showDialog<String>(
    context: context,
    builder: (d) => AlertDialog(
      title: Text(title),
      content: TextField(controller: c, obscureText: secret, autofocus: true, onSubmitted: (v) => Navigator.pop(d, v)),
      actions: [
        TextButton(onPressed: () => Navigator.pop(d), child: Text(tr('Cancel'))),
        FilledButton(onPressed: () => Navigator.pop(d, c.text), child: const Text('OK')),
      ],
    ),
  );
}

/// Switching two-step sign-in on: the key to type into an authenticator app (or open its link), then a code from it.
Future<void> _twoFactorDialog(BuildContext context, Api api, String secret, String uri) {
  final code = TextEditingController();
  String? err;
  return showDialog(
    context: context,
    builder: (ctx) => StatefulBuilder(
      builder: (ctx, set) => AlertDialog(
        title: Text(tr('Set up two-step sign-in')),
        content: SizedBox(
          width: 420,
          child: Column(mainAxisSize: MainAxisSize.min, crossAxisAlignment: CrossAxisAlignment.start, children: [
            Text(tr('Type this key into your authenticator app or open the link on this device:')),
            const SizedBox(height: 8),
            SelectableText(secret, style: const TextStyle(fontSize: 18, fontFamily: 'monospace')),
            Row(children: [
              TextButton(onPressed: () => Clipboard.setData(ClipboardData(text: secret)), child: Text(tr('Copy the key'))),
              TextButton(onPressed: () => launchUrl(Uri.parse(uri), mode: LaunchMode.externalApplication), child: Text(tr('Open in authenticator'))),
            ]),
            const SizedBox(height: 8),
            Text(tr('Then the 6-digit code it shows:')),
            TextField(controller: code, keyboardType: TextInputType.number, decoration: InputDecoration(hintText: '123456', errorText: err)),
          ]),
        ),
        actions: [
          TextButton(onPressed: () => Navigator.pop(ctx), child: Text(tr('Cancel'))),
          FilledButton(
            onPressed: () async {
              try {
                await api.post('/api/me/2fa/enable', {'code': code.text.trim()});
                if (ctx.mounted) Navigator.pop(ctx);
              } catch (e) {
                set(() => err = errorText(e));
              }
            },
            child: Text(tr('Switch on')),
          ),
        ],
      ),
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
      if (mounted) _say(context, errorText(e));
    }
  }

  @override
  Widget build(BuildContext context) {
    final m = me;
    if (m == null) return const Center(child: CircularProgressIndicator());
    return ListView(padding: const EdgeInsets.all(16), children: [
      _panel(context, tr('Your account'), [
        Text(tr('User name: {m}', {'m': m['username']})),
        const SizedBox(height: 12),
        TextField(controller: name, decoration: InputDecoration(labelText: tr('Full name'), border: OutlineInputBorder())),
        const SizedBox(height: 12),
        Align(
          alignment: Alignment.centerLeft,
          child: FilledButton(onPressed: () => run(() async => widget.api.post('/api/me/profile', {'full_name': name.text}), tr('Saved')), child: Text(tr('Save'))),
        ),
      ]),
      _panel(context, tr('Appearance'), [
        ListenableBuilder(
          listenable: AppTheme.changes,
          builder: (context, _) {
            // Kept with the account, so the web app shows the same.
            Future<void> save(Map<String, String> prefs) async {
              try {
                await widget.api.post('/api/me/prefs', prefs);
              } catch (_) {}
            }

            return Column(crossAxisAlignment: CrossAxisAlignment.start, children: [
              // The language is kept with the account too (pref "lang", as in the web app). Choosing one
              // builds the app again in it, so it is saved first.
              Row(children: [
                Text(tr('Language')),
                const SizedBox(width: 16),
                DropdownButton<String>(
                  value: I18n.lang.value,
                  items: [for (final e in I18n.names.entries) DropdownMenuItem(value: e.key, child: Text(e.value))],
                  onChanged: (v) async {
                    if (v == null || v == I18n.lang.value) return;
                    await save({'lang': v});
                    await I18n.set(v);
                  },
                ),
              ]),
              const SizedBox(height: 16),
              SegmentedButton<String>(
                showSelectedIcon: false,
                segments: [
                  ButtonSegment(value: 'system', icon: Icon(Icons.brightness_auto_outlined), label: Text(tr('System'))),
                  ButtonSegment(value: 'light', icon: Icon(Icons.light_mode_outlined), label: Text(tr('Light'))),
                  ButtonSegment(value: 'dark', icon: Icon(Icons.dark_mode_outlined), label: Text(tr('Dark'))),
                  ButtonSegment(value: 'oled', icon: Icon(Icons.contrast), label: Text('OLED')),
                ],
                selected: {AppTheme.choice.value},
                onSelectionChanged: (v) async {
                  await AppTheme.set(theme: v.first);
                  await save({'theme': v.first});
                },
              ),
              const SizedBox(height: 6),
              Text(tr("“System” follows the device's setting. “OLED” is pure black."), style: TextStyle(fontSize: 12, color: context.dim)),
              const SizedBox(height: 16),
              Text(tr('Accent colour')),
              const SizedBox(height: 8),
              Wrap(spacing: 12, children: [
                for (final e in AppTheme.accents.entries)
                  Semantics(
                    button: true,
                    selected: AppTheme.accent.value == e.key,
                    label: e.key,
                    child: InkWell(
                      customBorder: const CircleBorder(),
                      onTap: () async {
                        await AppTheme.set(accentName: e.key);
                        await save({'accent': e.key});
                      },
                      child: Container(
                        width: 36,
                        height: 36,
                        decoration: BoxDecoration(
                          shape: BoxShape.circle,
                          color: e.value[context.isDark ? 0 : 1],
                          border: Border.all(color: AppTheme.accent.value == e.key ? Theme.of(context).colorScheme.onSurface : context.hair2, width: AppTheme.accent.value == e.key ? 3 : 1),
                        ),
                      ),
                    ),
                  ),
              ]),
            ]);
          },
        ),
      ]),
      _panel(context, tr('Two-step sign-in'), [
        if (m['two_factor'] == true) ...[
          Text(tr('Two-step sign-in is on.')),
          const SizedBox(height: 12),
          Align(
            alignment: Alignment.centerLeft,
            child: OutlinedButton(
              onPressed: () async {
                final pw = await _askPassword(context, tr('Your password'));
                if (pw == null || pw.isEmpty || !context.mounted) return;
                final code = await _askPassword(context, tr('Code from the authenticator app'), secret: false);
                if (code == null || code.isEmpty) return;
                await run(() async {
                  await widget.api.post('/api/me/2fa/disable', {'password': pw, 'code': code});
                  final fresh = await widget.api.get('/api/me');
                  if (mounted) setState(() => me = Map<String, dynamic>.from(fresh));
                }, tr('Switched off'));
              },
              child: Text(tr('Switch off')),
            ),
          ),
        ] else ...[
          Text(tr('When signing in with a password the server also asks for a code from an authenticator app (Google Authenticator, Aegis, 1Password …).')),
          const SizedBox(height: 12),
          Align(
            alignment: Alignment.centerLeft,
            child: OutlinedButton(
              onPressed: () async {
                try {
                  final x = await widget.api.post('/api/me/2fa/setup');
                  if (!context.mounted) return;
                  await _twoFactorDialog(context, widget.api, '${x['secret']}', '${x['uri']}');
                  final fresh = await widget.api.get('/api/me');
                  if (mounted) setState(() => me = Map<String, dynamic>.from(fresh));
                } catch (e) {
                  if (context.mounted) _say(context, errorText(e));
                }
              },
              child: Text(tr('Set up')),
            ),
          ),
        ],
      ]),
      _panel(context, tr('Password'), [
        Align(
          alignment: Alignment.centerLeft,
          child: OutlinedButton(
            onPressed: () async {
              final old = await _askPassword(context, tr('Current password'));
              if (old == null || old.isEmpty || !context.mounted) return;
              final nw = await _askPassword(context, tr('New password (at least 6 characters)'));
              if (nw == null || nw.isEmpty) return;
              run(() async => widget.api.post('/api/me/password', {'old': old, 'new': nw}), tr('Password changed'));
            },
            child: Text(tr('Change password')),
          ),
        ),
      ]),
      PluginPanels(api: widget.api),
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
  static Map<String, String> get langs => {
    '': tr('Not set'),
    'en': tr('English'),
    'de': tr('German'),
    'fr': tr('French'),
    'es': tr('Spanish'),
    'it': tr('Italian'),
    'pt': tr('Portuguese'),
    'nl': tr('Dutch'),
    'sv': tr('Swedish'),
    'da': tr('Danish'),
    'no': tr('Norwegian'),
    'fi': tr('Finnish'),
    'pl': tr('Polish'),
    'ru': tr('Russian'),
    'tr': tr('Turkish'),
    'ja': tr('Japanese'),
    'ko': tr('Korean'),
    'zh': tr('Chinese'),
  };
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
          if (prefs['nightMode'] != null) await v.setBool('nightMode', '${prefs['nightMode']}' == '1');
        }
        // Older versions saved names the server does not know (1080, 720, ...): put the server's name in their place.
        final q = v.getString('quality');
        if (q != null && q != qualityKey(q)) {
          await v.setString('quality', qualityKey(q));
          await widget.api.post('/api/me/prefs', {'quality': qualityKey(q)});
        }
        final z = v.getInt('subSize');
        if (z != null && z != subSizeKey(z)) {
          await v.setInt('subSize', subSizeKey(z));
          await widget.api.post('/api/me/prefs', {'subSize': '${subSizeKey(z)}'});
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
              if (help.isNotEmpty) Text(help, style: TextStyle(fontSize: 12, color: context.dim)),
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
      _panel(context, tr('Quality'), [
        row(tr('Video quality'), drop('quality', 'original', playerQualities), tr('The quality a video starts with. Can be changed in the player.')),
        row(tr('Next episode by itself'), Switch(value: s.getBool('autoNext') ?? true, onChanged: (v) => save('autoNext', v)), tr('Start the next episode 10 seconds after one ends.')),
      ]),
      _panel(context, tr('Audio and subtitles'), [
        row(tr('Preferred audio language'), drop('audioLang', '', langs)),
        row(tr('Preferred subtitle language'), drop('subLang', '', langs)),
        row(
          tr('Subtitle size'),
          DropdownButton<int>(
            value: playerSubSizes.containsKey(s.getInt('subSize')) ? s.getInt('subSize') : 100,
            items: [for (final e in playerSubSizes.entries) DropdownMenuItem(value: e.key, child: Text(e.value))],
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
      if (mounted) setState(() => error = errorText(e));
    }
  }

  /// Swaps the token this app uses for a new one (the old one stops working); [all] also ends every other sign-in.
  Future<void> rotate(bool all) async {
    final ok = await showDialog<bool>(
      context: context,
      builder: (d) => AlertDialog(
        content: Text(all ? tr('Make a new token and sign out all other devices and scripts?') : tr('Make a new token? Scripts with the old token stop working.')),
        actions: [TextButton(onPressed: () => Navigator.pop(d, false), child: Text(tr('Cancel'))), FilledButton(onPressed: () => Navigator.pop(d, true), child: Text(tr('Yes')))],
      ),
    );
    if (ok != true) return;
    try {
      final r = await widget.api.post('/api/tokens/rotate', {'all': all});
      widget.api.token = r['token'];
      await widget.api.save();
      if (mounted) _say(context, all ? tr('New token; {x} other sign-in(s) ended', {'x': (r['revoked'] ?? 1) - 1}) : tr('New token; the old one no longer works'));
    } catch (e) {
      if (mounted) _say(context, errorText(e));
    }
    load();
  }

  @override
  Widget build(BuildContext context) {
    if (error != null) return Center(child: Text(error!));
    final l = tokens;
    if (l == null) return const Center(child: CircularProgressIndicator());
    if (l.isEmpty) return Center(child: Text(tr('No devices.')));
    return ListView(padding: const EdgeInsets.all(16), children: [
      Padding(padding: EdgeInsets.only(bottom: 8), child: Text(tr('Browsers, apps and scripts signed in to your account.'), style: TextStyle(color: context.dim))),
      Padding(
        padding: const EdgeInsets.only(bottom: 8),
        child: Wrap(spacing: 8, children: [
          OutlinedButton(onPressed: () => rotate(false), child: Text(tr('Renew the token'))),
          OutlinedButton(onPressed: () => rotate(true), child: Text(tr('Renew and sign out everywhere else'))),
        ]),
      ),
      for (final k in l)
        Card(
          child: ListTile(
            leading: const Icon(Icons.devices_other),
            title: Row(children: [
              Flexible(child: Text('${k['last_device'] ?? k['device'] ?? tr('Web app')}')),
              if (k['current'] == true) Padding(padding: EdgeInsets.only(left: 8), child: Chip(label: Text(tr('this one')), visualDensity: VisualDensity.compact)),
            ]),
            subtitle: Text(tr('Last used: {fmtDateTime} · address: {k} · signed in: {fmtDateTime2}', {'fmtDateTime': fmtDateTime(k['last_used']), 'k': k['last_ip'] ?? '–', 'fmtDateTime2': fmtDateTime(k['created_at'])})),
            trailing: k['current'] == true
                ? null
                : TextButton(
                    onPressed: () async {
                      final ok = await showDialog<bool>(
                        context: context,
                        builder: (d) => AlertDialog(
                          content: Text(tr('Sign this device out? It has to sign in again.')),
                          actions: [TextButton(onPressed: () => Navigator.pop(d, false), child: Text(tr('Cancel'))), FilledButton(onPressed: () => Navigator.pop(d, true), child: Text(tr('Sign out')))],
                        ),
                      );
                      if (ok != true) return;
                      try {
                        await widget.api.delete('/api/tokens/${k['id']}');
                      } catch (e) {
                        if (context.mounted) _say(context, errorText(e));
                      }
                      load();
                    },
                    child: Text(tr('Sign out')),
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
    try {
      final r = await widget.api.post('/api/rss', body);
      if (mounted && r is Map) setState(() => feed = Map<String, dynamic>.from(r));
    } catch (e) {
      if (mounted) _say(context, errorText(e));
    }
  }

  @override
  Widget build(BuildContext context) {
    final f = feed;
    if (f == null) return const Center(child: CircularProgressIndicator());
    final on = f['enabled'] == true;
    return ListView(padding: const EdgeInsets.all(16), children: [
      _panel(context, tr('Watchlist as a feed'), [
        Text(tr('Your watchlist as an RSS feed for a feed reader or an automation. Anyone who knows the address can read the list – so keep it private.'), style: TextStyle(color: context.dim)),
        const SizedBox(height: 12),
        if (on) ...[
          SelectableText('${f['url']}'),
          const SizedBox(height: 12),
          Wrap(spacing: 8, children: [
            OutlinedButton(
              onPressed: () {
                Clipboard.setData(ClipboardData(text: '${f['url']}'));
                _say(context, tr('Link copied'));
              },
              child: Text(tr('Copy the link')),
            ),
            OutlinedButton(onPressed: () => change({'regenerate': true}), child: Text(tr('New address'))),
            OutlinedButton(onPressed: () => change({'enabled': false}), child: Text(tr('Switch off'))),
          ]),
        ] else
          FilledButton(onPressed: () => change({'enabled': true}), child: Text(tr('Switch on'))),
      ]),
    ]);
  }
}
