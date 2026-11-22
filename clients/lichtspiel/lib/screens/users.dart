import 'package:flutter/material.dart';
import 'package:flutter/services.dart';
import '../api.dart';
import '../widgets/bar.dart';
import '../theme.dart';
import '../i18n.dart';

/// Admin: people, their rights, local users and invite links (the web app's "Users & invites").
class UsersScreen extends StatefulWidget {
  final Api api;
  const UsersScreen({super.key, required this.api});
  @override
  State<UsersScreen> createState() => _UsersScreenState();
}

class _UsersScreenState extends State<UsersScreen> {
  List<Map<String, dynamic>> users = [], invites = [], sections = [];
  String? error;
  bool loaded = false;
  Map<String, dynamic>? me;

  Api get api => widget.api;

  @override
  void initState() {
    super.initState();
    load();
  }

  List<Map<String, dynamic>> _list(dynamic r, String key) => ((r?[key] as List?) ?? const []).map((e) => Map<String, dynamic>.from(e)).toList();

  Future<void> load() async {
    try {
      final r = await Future.wait([api.get('/api/users'), api.get('/api/invites'), api.sections(), api.get('/api/me')]);
      if (!mounted) return;
      setState(() {
        users = _list(r[0], 'users');
        invites = _list(r[1], 'invites');
        sections = r[2] as List<Map<String, dynamic>>;
        me = Map<String, dynamic>.from(r[3] as Map);
        loaded = true;
      });
    } catch (e) {
      if (mounted) setState(() => error = '$e');
    }
  }

  void say(String s) {
    if (mounted) ScaffoldMessenger.of(context).showSnackBar(SnackBar(content: Text(s)));
  }

  Future<void> patch(Map<String, dynamic> u, Map<String, dynamic> body, {bool reload = false}) async {
    try {
      await api.request('PATCH', '/api/users/${u['id']}', body);
      if (reload) {
        await load();
      } else {
        say(tr('Saved'));
      }
    } catch (e) {
      say('$e');
      load();
    }
  }

  String secName(dynamic id) => '${sections.firstWhere((s) => '${s['key']}' == '$id', orElse: () => {'title': '#$id'})['title']}';

  String access(Map<String, dynamic> u) => u['admin'] == true
      ? tr('all (admin)')
      : u['allowed_sections'] == null
          ? tr('all')
          : (u['allowed_sections'] as List).map(secName).join(', ').isEmpty
              ? tr('none')
              : (u['allowed_sections'] as List).map(secName).join(', ');

  Future<bool> sure(String text) async =>
      await showDialog<bool>(
        context: context,
        builder: (d) => AlertDialog(
          content: Text(text),
          actions: [TextButton(onPressed: () => Navigator.pop(d, false), child: Text(tr('Cancel'))), FilledButton(onPressed: () => Navigator.pop(d, true), child: Text(tr('Yes')))],
        ),
      ) ??
      false;

  Future<String?> ask(String title, {bool password = false, String hint = ''}) {
    final c = TextEditingController();
    return showDialog<String>(
      context: context,
      builder: (d) => AlertDialog(
        title: Text(title),
        content: TextField(controller: c, obscureText: password, autofocus: true, decoration: InputDecoration(helperText: hint.isEmpty ? null : hint), onSubmitted: (v) => Navigator.pop(d, v)),
        actions: [TextButton(onPressed: () => Navigator.pop(d), child: Text(tr('Cancel'))), FilledButton(onPressed: () => Navigator.pop(d, c.text), child: const Text('OK'))],
      ),
    );
  }

  /// Which libraries: null means all, including future ones.
  Future<List<dynamic>?> pickSections(String title, List<dynamic>? current, {bool cancellable = true}) async {
    var all = current == null;
    final on = <String>{for (final s in sections) if (current == null || current.map((e) => '$e').contains('${s['key']}')) '${s['key']}'};
    final ok = await showDialog<bool>(
      context: context,
      builder: (d) => StatefulBuilder(
        builder: (d, set) => AlertDialog(
          title: Text(title),
          content: SizedBox(
            width: 360,
            child: ListView(shrinkWrap: true, children: [
              CheckboxListTile(value: all, title: Text(tr('All, also future ones')), onChanged: (v) => set(() => all = v ?? false)),
              for (final s in sections)
                CheckboxListTile(
                  value: all || on.contains('${s['key']}'),
                  title: Text('${s['title']}'),
                  onChanged: all ? null : (v) => set(() => v == true ? on.add('${s['key']}') : on.remove('${s['key']}')),
                ),
            ]),
          ),
          actions: [TextButton(onPressed: () => Navigator.pop(d, false), child: Text(tr('Cancel'))), FilledButton(onPressed: () => Navigator.pop(d, true), child: const Text('OK'))],
        ),
      ),
    );
    if (ok != true) return current == null && !cancellable ? null : (cancellable ? [-1] : current);
    return all ? null : [for (final s in sections) if (on.contains('${s['key']}')) s['key']];
  }

  Future<void> addLocal() async {
    final name = TextEditingController(), pass = TextEditingController(), pin = TextEditingController();
    var admin = false, home = false;
    String? err;
    await showDialog<void>(
      context: context,
      builder: (d) => StatefulBuilder(
        builder: (d, set) => AlertDialog(
          title: Text(tr('Add a local user')),
          content: SizedBox(
            width: 380,
            child: SingleChildScrollView(
              child: Column(mainAxisSize: MainAxisSize.min, children: [
                TextField(controller: name, decoration: InputDecoration(labelText: tr('User name'))),
                TextField(controller: pass, obscureText: true, decoration: InputDecoration(labelText: tr('Password (at least 6 characters)'), helperText: tr('A home profile may leave it empty.'))),
                TextField(controller: pin, obscureText: true, keyboardType: TextInputType.number, decoration: InputDecoration(labelText: tr('PIN (optional)'), helperText: tr('4 to 8 digits. Admins need one.'))),
                SwitchListTile(value: home, title: Text(tr('Home profile')), onChanged: (v) => set(() => home = v)),
                SwitchListTile(value: admin, title: Text(tr('Admin')), onChanged: (v) => set(() => admin = v)),
                if (err != null) Text(err!, style: TextStyle(color: context.bad)),
              ]),
            ),
          ),
          actions: [
            TextButton(onPressed: () => Navigator.pop(d), child: Text(tr('Cancel'))),
            FilledButton(
              onPressed: () async {
                try {
                  await api.post('/api/users', {'username': name.text, 'password': pass.text, 'admin': admin, 'home': home, 'pin': pin.text});
                  if (d.mounted) Navigator.pop(d);
                  say(tr('{name} was added', {'name': name.text.trim()}));
                  load();
                } catch (e) {
                  set(() => err = '$e');
                }
              },
              child: Text(tr('Add')),
            ),
          ],
        ),
      ),
    );
  }

  Future<void> newInvite() async {
    final email = TextEditingController(), note = TextEditingController(), days = TextEditingController(text: '7');
    var admin = false;
    final ok = await showDialog<bool>(
      context: context,
      builder: (d) => StatefulBuilder(
        builder: (d, set) => AlertDialog(
          title: Text(tr('Invite someone')),
          content: SizedBox(
            width: 380,
            child: SingleChildScrollView(
              child: Column(mainAxisSize: MainAxisSize.min, children: [
                TextField(controller: email, decoration: InputDecoration(labelText: tr('Email (optional)'))),
                TextField(controller: note, decoration: InputDecoration(labelText: tr('Message (optional)'))),
                TextField(controller: days, keyboardType: TextInputType.number, decoration: InputDecoration(labelText: tr('Link valid for (days)'))),
                SwitchListTile(value: admin, title: Text(tr('Make an admin')), onChanged: (v) => set(() => admin = v)),
              ]),
            ),
          ),
          actions: [TextButton(onPressed: () => Navigator.pop(d, false), child: Text(tr('Cancel'))), FilledButton(onPressed: () => Navigator.pop(d, true), child: Text(tr('Create the link')))],
        ),
      ),
    );
    if (ok != true || !mounted) return;
    final libs = await pickSections(tr('Libraries for the invite'), null);
    if (!mounted || (libs != null && libs.length == 1 && libs.first == -1)) return;
    try {
      final r = await api.post('/api/invites', {'email': email.text, 'note': note.text, 'days': int.tryParse(days.text) ?? 7, 'admin': admin, 'allowed_sections': ?libs});
      await Clipboard.setData(ClipboardData(text: '${r['url']}'));
      if (!mounted) return;
      await showDialog<void>(
        context: context,
        builder: (d) => AlertDialog(
          title: Text(tr('Invite ready')),
          content: Column(mainAxisSize: MainAxisSize.min, crossAxisAlignment: CrossAxisAlignment.start, children: [
            Text(tr('This link is on the clipboard. Send it on:')),
            const SizedBox(height: 8),
            SelectableText('${r['url']}'),
          ]),
          actions: [FilledButton(onPressed: () => Navigator.pop(d), child: const Text('OK'))],
        ),
      );
      load();
    } catch (e) {
      say('$e');
    }
  }

  Widget userCard(Map<String, dynamic> u) {
    final self = u['id'] == me?['id'];
    final admin = u['admin'] == true;
    return Card(
      child: Padding(
        padding: const EdgeInsets.all(12),
        child: Column(crossAxisAlignment: CrossAxisAlignment.start, children: [
          Row(children: [
            Text('${u['display_name'] ?? u['username']}', style: const TextStyle(fontWeight: FontWeight.bold, fontSize: 16)),
            if ((u['display_name'] ?? u['username']) != u['username']) Padding(padding: const EdgeInsets.only(left: 8), child: Text('${u['username']}', style: TextStyle(color: context.dim))),
            if (self) Padding(padding: EdgeInsets.only(left: 8), child: Text(tr('(you)'), style: TextStyle(color: context.dim))),
            if (u['two_factor'] == true) const Padding(padding: EdgeInsets.only(left: 8), child: Icon(Icons.lock_outline, size: 16)),
            const Spacer(),
            if (!self) ...[
              TextButton(
                onPressed: () async {
                  final pw = await ask(tr('New password for {u}', {'u': u['username']}), password: true);
                  if (pw != null && pw.isNotEmpty) {
                    try {
                      await api.request('PATCH', '/api/users/${u['id']}', {'password': pw});
                      say(tr('Password changed'));
                    } catch (e) {
                      say('$e');
                    }
                  }
                },
                child: Text(tr('Password')),
              ),
              TextButton(
                onPressed: () async {
                  if (await sure(tr('Remove {u}?', {'u': u['username']}))) {
                    try {
                      await api.delete('/api/users/${u['id']}');
                      load();
                    } catch (e) {
                      say('$e');
                    }
                  }
                },
                child: Text(tr('Remove'), style: TextStyle(color: context.bad)),
              ),
            ],
          ]),
          if ('${u['email'] ?? ''}'.isNotEmpty) Text('${u['email']}', style: TextStyle(color: context.dim)),
          const SizedBox(height: 4),
          Wrap(spacing: 4, runSpacing: 0, crossAxisAlignment: WrapCrossAlignment.center, children: [
            Text(tr('Libraries: {access}', {'access': access(u)})),
            if (!admin)
              TextButton(
                onPressed: () async {
                  final r = await pickSections(tr('Libraries for {u}', {'u': u['username']}), u['allowed_sections'] as List?);
                  if (r != null && r.length == 1 && r.first == -1) return;
                  patch(u, {'allowed_sections': r}, reload: true);
                },
                child: Text(tr('Change')),
              ),
          ]),
          Wrap(spacing: 16, children: [
            _check(tr('Admin'), admin, self ? null : (v) => patch(u, {'admin': v}, reload: true)),
            _check(tr('May convert'), u['allow_transcode'] == true, (v) => patch(u, {'allow_transcode': v}, reload: true)),
            _check(tr('May delete files'), admin || u['allow_delete'] == true, admin ? null : (v) => patch(u, {'allow_delete': v}, reload: true)),
            _check(tr('May download'), admin || u['allow_download'] == true, admin ? null : (v) => patch(u, {'allow_download': v}, reload: true)),
            _check(tr('Home profile'), u['home'] == true, (v) => patch(u, {'home': v}, reload: true)),
          ]),
          Wrap(spacing: 12, crossAxisAlignment: WrapCrossAlignment.center, children: [
            if (!admin) ...[
              Text(tr('Age limit:')),
              DropdownButton<int?>(
                value: u['max_age'] as int?,
                items: [DropdownMenuItem(value: null, child: Text(tr('None'))), for (final a in [0, 6, 12, 16, 18]) DropdownMenuItem(value: a, child: Text(tr('up to {a}', {'a': a})))],
                onChanged: (v) => patch(u, {'max_age': v}, reload: true),
              ),
              if (u['max_age'] != null) _check(tr('Hide unrated'), u['block_unrated'] == true, (v) => patch(u, {'block_unrated': v}, reload: true)),
            ],
            TextButton(
              onPressed: () async {
                final pin = await ask(tr('PIN for {u}', {'u': u['username']}), password: true, hint: tr('4 to 8 digits. Leave empty to remove the PIN.'));
                if (pin != null) patch(u, {'pin': pin}, reload: true);
              },
              child: Text(u['has_pin'] == true ? tr('PIN 🔒') : 'PIN'),
            ),
          ]),
        ]),
      ),
    );
  }

  Widget _check(String label, bool v, void Function(bool)? on) => Row(mainAxisSize: MainAxisSize.min, children: [
        Checkbox(value: v, onChanged: on == null ? null : (x) => on(x ?? false)),
        Text(label),
      ]);

  String _date(dynamic s) {
    if (s is! num) return '';
    final d = DateTime.fromMillisecondsSinceEpoch((s * 1000).round());
    return fmtDay(d, pad: true);
  }

  @override
  Widget build(BuildContext context) {
    return Scaffold(
      appBar: AppBar(title: Text(tr('Users & invites')), actions: barActions(context, labels: true, keep: 2, [
        BarAction(Icons.person_add_alt, tr('Local user'), addLocal),
        BarAction(Icons.mail_outline, tr('Invite'), newInvite),
      ])),
      body: error != null
          ? Center(child: Text(error!))
          : !loaded
              ? const Center(child: CircularProgressIndicator())
              : ListView(padding: const EdgeInsets.all(16), children: [
                  Text(tr('People'), style: Theme.of(context).textTheme.titleMedium),
                  const SizedBox(height: 8),
                  for (final u in users) userCard(u),
                  const SizedBox(height: 16),
                  Text(tr('Invites'), style: Theme.of(context).textTheme.titleMedium),
                  const SizedBox(height: 8),
                  if (invites.isEmpty) Text(tr('No invites.'), style: TextStyle(color: context.dim)),
                  for (final i in invites)
                    Card(
                      child: ListTile(
                        title: Text('${i['email'] ?? ''}'.isEmpty ? tr('anyone with the link') : '${i['email']}'),
                        subtitle: Text([
                          i['status'] == 'used' ? tr('used by {i}', {'i': i['used_by']}) : i['status'] == 'open' ? tr('open until {date}', {'date': _date(i['expires_at'])}) : tr('expired'),
                          if ('${i['note'] ?? ''}'.isNotEmpty) '${i['note']}',
                        ].join(' · ')),
                        trailing: Row(mainAxisSize: MainAxisSize.min, children: [
                          if (i['status'] == 'open')
                            IconButton(
                              tooltip: tr('Copy the link'),
                              icon: const Icon(Icons.copy),
                              onPressed: () {
                                Clipboard.setData(ClipboardData(text: '${i['url']}'));
                                say(tr('Link copied'));
                              },
                            ),
                          IconButton(
                            tooltip: i['status'] == 'open' ? tr('Withdraw the invite') : tr('Delete'),
                            icon: const Icon(Icons.delete_outline),
                            onPressed: () async {
                              if (!await sure(i['status'] == 'open' ? tr('Withdraw this invite?') : tr('Delete this invite?'))) return;
                              try {
                                await api.delete('/api/invites/${i['code']}');
                              } catch (e) {
                                say('$e');
                              }
                              load();
                            },
                          ),
                        ]),
                      ),
                    ),
                ]),
    );
  }
}
