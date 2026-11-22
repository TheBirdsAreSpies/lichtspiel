import 'dart:async';
import 'package:flutter/material.dart';
import 'package:flutter/services.dart';
import '../api.dart';
import '../screens/player.dart';
import '../theme.dart';
import 'states.dart';
import '../i18n.dart';

/// "Zusammen schauen": opens a room for [item]; people registered here can be ticked and invited,
/// or the link/code sent. "Starten" joins it.
Future<void> watchTogether(BuildContext context, Api api, Map<String, dynamic> item) async {
  final id = '${item['ratingKey'] ?? item['id']}';
  try {
    final r = await api.post('/api/together', {'item': id, 'position': item['viewOffset'] ?? 0});
    final code = '${r['code']}';
    final link = '${api.base}/#/together/$code';
    if (!context.mounted) return;
    await showDialog(
      context: context,
      builder: (ctx) => AlertDialog(
        title: Text(tr('Watch together')),
        content: SingleChildScrollView(
          child: Column(mainAxisSize: MainAxisSize.min, crossAxisAlignment: CrossAxisAlignment.start, children: [
            Text(tr('Whoever plays, pauses or skips takes everyone along.'), style: TextStyle(color: context.faint)),
            const SizedBox(height: 12),
            Text(tr('People here'), style: TextStyle(fontWeight: FontWeight.bold)),
            TogetherPeople(api: api, code: code),
            const SizedBox(height: 12),
            Text(tr('Or send the link'), style: TextStyle(fontWeight: FontWeight.bold)),
            Text(tr('For someone with an account here who may see this library.'), style: TextStyle(color: context.faint)),
            const SizedBox(height: 6),
            SelectableText(link),
            Text(tr('Code: {code}', {'code': code})),
          ]),
        ),
        actions: [
          TextButton(onPressed: () => Clipboard.setData(ClipboardData(text: link)), child: Text(tr('Copy the link'))),
          TextButton(onPressed: () => Navigator.pop(ctx), child: Text(tr('Close'))),
          FilledButton(
            onPressed: () {
              Navigator.pop(ctx);
              joinTogetherWith(context, api, code);
            },
            child: Text(tr('Start')),
          ),
        ],
      ),
    );
  } catch (e) {
    if (context.mounted) ScaffoldMessenger.of(context).showSnackBar(SnackBar(content: Text(errorText(e))));
  }
}

/// Opens the player in the room [code]; the player takes video, place and pause state from the server.
Future<void> joinTogetherWith(BuildContext context, Api api, String code) async {
  try {
    final st = await api.get('/api/together/$code');
    if (!context.mounted) return;
    TogetherInvites.inRoom = true;
    try {
      await Navigator.push(context, MaterialPageRoute(builder: (_) => PlayerScreen(api: api, item: {'id': st['item'], 'ratingKey': '${st['item']}', 'viewOffset': st['position']}, together: code)));
    } finally {
      TogetherInvites.inRoom = false;
    }
  } catch (e) {
    if (context.mounted) ScaffoldMessenger.of(context).showSnackBar(SnackBar(content: Text(errorText(e))));
  }
}

/// The people registered here who may see the room's video: tick them and "Einladen";
/// they get the invitation in the app.
class TogetherPeople extends StatefulWidget {
  final Api api;
  final String code;
  const TogetherPeople({super.key, required this.api, required this.code});
  @override
  State<TogetherPeople> createState() => _TogetherPeopleState();
}

class _TogetherPeopleState extends State<TogetherPeople> {
  List<Map<String, dynamic>>? people;
  String? error;
  final picked = <int>{};

  @override
  void initState() {
    super.initState();
    widget.api.get('/api/together/${widget.code}/people').then((r) {
      if (mounted) setState(() => people = [for (final p in (r['people'] as List? ?? [])) if (p['in_room'] != true) Map<String, dynamic>.from(p)]);
    }).catchError((e) {
      if (mounted) setState(() => error = errorText(e));
    });
  }

  Future<void> invite() async {
    final ids = picked.toList();
    try {
      await widget.api.post('/api/together/${widget.code}/invite', {'users': ids});
      if (!mounted) return;
      setState(() {
        for (final p in people!) {
          if (ids.contains(p['id'])) p['invited'] = true;
        }
        picked.clear();
      });
    } catch (e) {
      if (mounted) ScaffoldMessenger.of(context).showSnackBar(SnackBar(content: Text(errorText(e))));
    }
  }

  @override
  Widget build(BuildContext context) {
    if (error != null) return Text(error!, style: TextStyle(color: Theme.of(context).colorScheme.error));
    final list = people;
    if (list == null) return const Padding(padding: EdgeInsets.all(8), child: LinearProgressIndicator());
    if (list.isEmpty) return Text(tr('Nobody else here may see this title.'), style: TextStyle(color: context.faint));
    return Column(crossAxisAlignment: CrossAxisAlignment.start, children: [
      for (final p in list)
        CheckboxListTile(
          dense: true,
          contentPadding: EdgeInsets.zero,
          controlAffinity: ListTileControlAffinity.leading,
          value: p['invited'] == true || picked.contains(p['id']),
          onChanged: p['invited'] == true ? null : (v) => setState(() => v == true ? picked.add(p['id'] as int) : picked.remove(p['id'])),
          title: Text('${p['name']}'),
          subtitle: p['invited'] == true ? Text(tr('invited')) : p['online'] == true ? const Text('online') : null,
        ),
      FilledButton.tonal(onPressed: picked.isEmpty ? null : invite, child: Text(tr('Invite'))),
    ]);
  }
}

/// Invitations from others here: looked for every 15 seconds while the app is open, each shown once.
class TogetherInvites {
  static Timer? _timer;
  static bool inRoom = false;
  static bool _asking = false;
  static final _seen = <String>{};

  static void start(BuildContext context, Api api) {
    _timer?.cancel();
    _timer = Timer.periodic(const Duration(seconds: 15), (_) => _check(context, api));
  }

  static void stop() {
    _timer?.cancel();
    _timer = null;
  }

  static Future<void> _check(BuildContext context, Api api) async {
    if (inRoom || _asking || !context.mounted) return;
    Map? inv;
    try {
      final r = await api.get('/api/together/invites');
      for (final i in (r['invites'] as List? ?? [])) {
        if (!_seen.contains('${i['code']}')) {
          inv = i as Map;
          break;
        }
      }
    } catch (_) {
      return;
    }
    if (inv == null || !context.mounted || inRoom) return;
    final code = '${inv['code']}';
    _seen.add(code);
    final others = [for (final n in (inv['members'] as List? ?? [])) if (n != inv['by']) '$n'];
    _asking = true;
    final join = await showDialog<bool>(
      context: context,
      builder: (ctx) => AlertDialog(
        title: Text(tr('Watch together')),
        content: Column(mainAxisSize: MainAxisSize.min, crossAxisAlignment: CrossAxisAlignment.start, children: [
          Text(tr('{inv} invites you to watch “{inv2}” together.', {'inv': inv!['by'], 'inv2': inv['title']})),
          if (others.isNotEmpty) Padding(padding: const EdgeInsets.only(top: 8), child: Text(tr('Already in: {others}', {'others': others.join(', ')}), style: TextStyle(color: ctx.faint))),
        ]),
        actions: [
          TextButton(onPressed: () => Navigator.pop(ctx, false), child: Text(tr('No, thanks'))),
          FilledButton(onPressed: () => Navigator.pop(ctx, true), child: Text(tr('Join'))),
        ],
      ),
    );
    _asking = false;
    if (!context.mounted) return;
    if (join == true) {
      await joinTogetherWith(context, api, code);
    } else if (join == false) {
      api.delete('/api/together/$code/invite').catchError((_) {});
    }
  }
}

/// Asks for a room's link or code and joins it.
Future<void> joinTogetherDialog(BuildContext context, Api api) async {
  final c = TextEditingController();
  final text = await showDialog<String>(
    context: context,
    builder: (ctx) => AlertDialog(
      title: Text(tr('Join a room')),
      content: TextField(controller: c, autofocus: true, decoration: InputDecoration(labelText: tr('Link or code')), onSubmitted: (v) => Navigator.pop(ctx, v)),
      actions: [
        TextButton(onPressed: () => Navigator.pop(ctx), child: Text(tr('Cancel'))),
        FilledButton(onPressed: () => Navigator.pop(ctx, c.text), child: Text(tr('Join'))),
      ],
    ),
  );
  c.dispose();
  final code = (text ?? '').trim().split('/').last.trim();
  if (code.isEmpty || !context.mounted) return;
  await joinTogetherWith(context, api, code);
}
