import 'dart:async';
import 'package:flutter/material.dart';
import 'package:flutter/services.dart';
import '../api.dart';
import '../screens/player.dart';
import '../theme.dart';
import 'states.dart';

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
        title: const Text('Zusammen schauen'),
        content: SingleChildScrollView(
          child: Column(mainAxisSize: MainAxisSize.min, crossAxisAlignment: CrossAxisAlignment.start, children: [
            Text('Wer abspielt, pausiert oder springt, nimmt alle mit.', style: TextStyle(color: context.faint)),
            const SizedBox(height: 12),
            const Text('Leute hier', style: TextStyle(fontWeight: FontWeight.bold)),
            TogetherPeople(api: api, code: code),
            const SizedBox(height: 12),
            const Text('Oder den Link schicken', style: TextStyle(fontWeight: FontWeight.bold)),
            Text('Für jemanden mit einem Konto hier, das diese Mediathek sehen darf.', style: TextStyle(color: context.faint)),
            const SizedBox(height: 6),
            SelectableText(link),
            Text('Code: $code'),
          ]),
        ),
        actions: [
          TextButton(onPressed: () => Clipboard.setData(ClipboardData(text: link)), child: const Text('Link kopieren')),
          TextButton(onPressed: () => Navigator.pop(ctx), child: const Text('Schließen')),
          FilledButton(
            onPressed: () {
              Navigator.pop(ctx);
              joinTogetherWith(context, api, code);
            },
            child: const Text('Starten'),
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
    if (list.isEmpty) return Text('Sonst darf hier niemand diesen Titel sehen.', style: TextStyle(color: context.faint));
    return Column(crossAxisAlignment: CrossAxisAlignment.start, children: [
      for (final p in list)
        CheckboxListTile(
          dense: true,
          contentPadding: EdgeInsets.zero,
          controlAffinity: ListTileControlAffinity.leading,
          value: p['invited'] == true || picked.contains(p['id']),
          onChanged: p['invited'] == true ? null : (v) => setState(() => v == true ? picked.add(p['id'] as int) : picked.remove(p['id'])),
          title: Text('${p['name']}'),
          subtitle: p['invited'] == true ? const Text('eingeladen') : p['online'] == true ? const Text('online') : null,
        ),
      FilledButton.tonal(onPressed: picked.isEmpty ? null : invite, child: const Text('Einladen')),
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
        title: const Text('Zusammen schauen'),
        content: Column(mainAxisSize: MainAxisSize.min, crossAxisAlignment: CrossAxisAlignment.start, children: [
          Text('${inv!['by']} lädt dich ein, „${inv['title']}“ gemeinsam anzusehen.'),
          if (others.isNotEmpty) Padding(padding: const EdgeInsets.only(top: 8), child: Text('Schon dabei: ${others.join(', ')}', style: TextStyle(color: ctx.faint))),
        ]),
        actions: [
          TextButton(onPressed: () => Navigator.pop(ctx, false), child: const Text('Nein, danke')),
          FilledButton(onPressed: () => Navigator.pop(ctx, true), child: const Text('Beitreten')),
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
      title: const Text('Einem Raum beitreten'),
      content: TextField(controller: c, autofocus: true, decoration: const InputDecoration(labelText: 'Link oder Code'), onSubmitted: (v) => Navigator.pop(ctx, v)),
      actions: [
        TextButton(onPressed: () => Navigator.pop(ctx), child: const Text('Abbrechen')),
        FilledButton(onPressed: () => Navigator.pop(ctx, c.text), child: const Text('Beitreten')),
      ],
    ),
  );
  c.dispose();
  final code = (text ?? '').trim().split('/').last.trim();
  if (code.isEmpty || !context.mounted) return;
  await joinTogetherWith(context, api, code);
}
