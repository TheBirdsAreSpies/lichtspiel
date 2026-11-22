import 'dart:async';
import 'dart:io';
import 'package:flutter/material.dart';
import 'package:http/http.dart' as http;
import 'package:path_provider/path_provider.dart';
import 'package:url_launcher/url_launcher.dart';
import '../api.dart';
import '../widgets/poster.dart';
import 'detail.dart' show fmtSize;
import '../theme.dart';
import '../widgets/states.dart';
import '../i18n.dart';

Map<String, String> get dlQualities => {
  'original': tr('Original quality'),
  '1080p-8': '1080p 8 Mbit/s',
  '720p-4': '720p 4 Mbit/s',
  '720p-3': '720p 3 Mbit/s',
  '720p-2': '720p 2 Mbit/s',
  '480p-1.5': tr('480p 1.5 Mbit/s'),
  '320p-0.7': tr('320p 0.7 Mbit/s'),
};

/// Whether the signed-in person may download / have videos converted (set by the home screen from /api/me).
class DownloadRights {
  static bool allowed = false;
  static bool convert = true;
}

/// Saving a file. On iPhone, iPad and Mac the app loads it itself (iPhone/iPad: into the app's folder,
/// which the Files app shows under "Auf diesem Gerät › Lichtspiel"; Mac: into the Downloads folder).
/// Elsewhere the system browser downloads it (the link carries a short-lived ticket, never the token).
Future<void> saveFile(BuildContext context, Api api, String url) async {
  if (Platform.isIOS || Platform.isMacOS) return downloadToFile(context, api, url);
  String link;
  try {
    link = await api.ticketLink(url);
  } on ApiError catch (e) {
    if (context.mounted) ScaffoldMessenger.of(context).showSnackBar(SnackBar(content: Text(e.message)));
    return;
  }
  if (!await launchUrl(Uri.parse(link), mode: LaunchMode.externalApplication) && context.mounted) {
    ScaffoldMessenger.of(context).showSnackBar(SnackBar(content: Text(tr('The file could not be opened'))));
  }
}

/// The file name from the server's Content-Disposition header (filename*=UTF-8''… first).
String? fileNameFrom(String? disposition) {
  if (disposition == null) return null;
  final star = RegExp(r"filename\*=UTF-8''([^;]+)", caseSensitive: false).firstMatch(disposition);
  if (star != null) {
    try {
      return Uri.decodeComponent(star[1]!.trim());
    } catch (_) {}
  }
  return RegExp(r'filename="([^"]+)"', caseSensitive: false).firstMatch(disposition)?[1];
}

/// Downloads into the Downloads folder (Documents on iOS) with a progress dialog.
@visibleForTesting
Future<void> downloadToFile(BuildContext context, Api api, String url) async {
  final messenger = ScaffoldMessenger.of(context);
  final progress = ValueNotifier<double?>(null);
  final client = http.Client();
  var cancelled = false, finished = false;
  BuildContext? dialogContext;
  final dialog = showDialog<void>(
    context: context,
    barrierDismissible: false,
    builder: (d) {
      dialogContext = d;
      return AlertDialog(
      title: Text(tr('Downloading…')),
      content: ValueListenableBuilder<double?>(
        valueListenable: progress,
        builder: (_, v, _) => Column(mainAxisSize: MainAxisSize.min, children: [
          LinearProgressIndicator(value: v),
          if (v != null) Padding(padding: const EdgeInsets.only(top: 8), child: Text('${(v * 100).toStringAsFixed(0)} %')),
          Padding(padding: EdgeInsets.only(top: 8), child: Text(tr('Keep the app open meanwhile.'), style: TextStyle(color: context.faint))),
        ]),
      ),
      actions: [
        TextButton(
          onPressed: () => Navigator.pop(d),
          child: Text(tr('Cancel')),
        ),
      ],
    );
    },
  );
  // However the dialog goes away (the button, the mouse's back button), the download stops.
  dialog.whenComplete(() {
    if (!finished) {
      cancelled = true;
      client.close();
    }
  });
  File? out;
  try {
    final dir = Platform.isIOS ? await getApplicationDocumentsDirectory() : (await getDownloadsDirectory() ?? await getApplicationDocumentsDirectory());
    final r = await client.send(http.Request('GET', Uri.parse(api.abs(url)))..headers.addAll(api.auth ?? {}));
    if (r.statusCode >= 400) throw tr('Error {statusCode}', {'statusCode': r.statusCode});
    final name = (fileNameFrom(r.headers['content-disposition']) ?? 'lichtspiel-download').replaceAll(RegExp(r'[/\\:]'), '_');
    // Don't overwrite a file of the same name: "Film (2).mkv".
    var f = File('${dir.path}/$name');
    final dot = name.lastIndexOf('.');
    for (var n = 2; await f.exists(); n++) {
      f = File('${dir.path}/${dot > 0 ? '${name.substring(0, dot)} ($n)${name.substring(dot)}' : '$name ($n)'}');
    }
    // Written under "….part" and renamed at the end, so a cut-off download never looks finished.
    out = File('${f.path}.part');
    final total = r.contentLength ?? 0;
    var got = 0;
    final sink = out.openWrite();
    try {
      await for (final chunk in r.stream) {
        sink.add(chunk);
        got += chunk.length;
        if (total > 0) progress.value = got / total;
      }
    } finally {
      await sink.close();
    }
    if (cancelled) throw tr('Cancelled');
    // A connection that ends early can close the stream without an error.
    if (total > 0 && got < total) throw tr('Connection lost ({fmtSize} of {fmtSize2})', {'fmtSize': fmtSize(got), 'fmtSize2': fmtSize(total)});
    await out.rename(f.path);
    out = null;
    messenger.showSnackBar(SnackBar(content: Text(Platform.isIOS ? tr('Saved: “{f}” (Files app › On My iPhone › Lichtspiel)', {'f': f.path.split('/').last}) : tr('Saved in “Downloads”: {f}', {'f': f.path.split('/').last}))));
  } catch (e) {
    if (out != null && await out.exists()) {
      try {
        await out.delete();
      } catch (_) {}
    }
    if (!cancelled) messenger.showSnackBar(SnackBar(content: Text(tr('Download failed: {e}', {'e': e}))));
  } finally {
    finished = true;
    client.close();
    // A download that fails at once can end before the dialog's first frame, when it has no
    // context yet; wait for that frame so the dialog can still be closed.
    for (var i = 0; dialogContext == null && i < 10; i++) {
      await WidgetsBinding.instance.endOfFrame;
    }
    final d = dialogContext;
    if (!cancelled && d != null && d.mounted) Navigator.pop(d); // only the dialog, never the screen behind it
    await dialog;
    progress.dispose();
  }
}

/// "Herunterladen…" for one or several movies, episodes, seasons or shows.
Future<void> downloadDialog(BuildContext context, Api api, List<Map<String, dynamic>> items) async {
  final messenger = ScaffoldMessenger.of(context);
  final nav = Navigator.of(context);
  if (!DownloadRights.allowed) {
    messenger.showSnackBar(SnackBar(content: Text(tr('Downloads are not allowed'))));
    return;
  }
  var q = 'original';
  final title = items.length == 1 ? '${items.first['title']}' : tr('{length} selected', {'length': items.length});
  final go = await showDialog<bool>(
    context: context,
    builder: (ctx) => StatefulBuilder(
      builder: (ctx, set) => AlertDialog(
        title: Text(tr('Download “{title}”', {'title': title})),
        content: Column(mainAxisSize: MainAxisSize.min, crossAxisAlignment: CrossAxisAlignment.start, children: [
          DropdownButton<String>(
            value: q,
            isExpanded: true,
            onChanged: (v) => set(() => q = v ?? 'original'),
            items: [for (final e in dlQualities.entries) DropdownMenuItem(value: e.key, enabled: e.key == 'original' || DownloadRights.convert, child: Text(e.value))],
          ),
          const SizedBox(height: 8),
          Text(
            q == 'original'
                ? tr('The file as it is on the server. It is ready at once.')
                : tr('Converted on the server to MP4 (H.264, stereo), which plays everywhere. You find it under “Downloads” with its progress and download it from there when it is ready. A file that is small enough already comes unchanged.'),
            style: TextStyle(color: context.faint),
          ),
          if (!DownloadRights.convert)
            Padding(padding: EdgeInsets.only(top: 8), child: Text(tr('You may not have videos converted, so only the original quality is possible.'), style: TextStyle(color: context.faint))),
        ]),
        actions: [
          TextButton(onPressed: () => Navigator.pop(ctx, false), child: Text(tr('Cancel'))),
          FilledButton(onPressed: () => Navigator.pop(ctx, true), child: Text(tr('Download'))),
        ],
      ),
    ),
  );
  if (go != true) return;
  try {
    final r = await api.post('/api/downloads', {
      'ids': [for (final i in items) '${i['ratingKey'] ?? i['id']}'],
      'quality': q,
    });
    num n(String k) => (r[k] ?? 0) as num;
    final parts = [
      if (n('ready') > 0) tr('{n} ready', {'n': n('ready')}),
      if (n('queued') > 0) tr('{n} being converted', {'n': n('queued')}),
      if (n('already') > 0) tr('{n} already there', {'n': n('already')}),
      if (n('refused') > 0) tr('{n} left out (you may not convert)', {'n': n('refused')}),
    ];
    messenger.showSnackBar(SnackBar(
      content: Text(tr('Downloads: {parts}', {'parts': parts.join(', ')})),
      action: SnackBarAction(label: tr('Open'), onPressed: () => nav.push(MaterialPageRoute(builder: (_) => DownloadsScreen(api: api)))),
    ));
  } catch (e) {
    messenger.showSnackBar(SnackBar(content: Text(errorText(e))));
  }
}

class DownloadsScreen extends StatefulWidget {
  final Api api;
  const DownloadsScreen({super.key, required this.api});
  @override
  State<DownloadsScreen> createState() => _DownloadsScreenState();
}

class _DownloadsScreenState extends State<DownloadsScreen> {
  static Map<String, String> get states => {'queued': tr('Waiting'), 'converting': tr('Converting'), 'ready': tr('Done'), 'failed': tr('Failed')};
  Map<String, dynamic>? data;
  String? error;
  bool all = false;
  Timer? timer;

  @override
  void initState() {
    super.initState();
    reload();
  }

  @override
  void dispose() {
    timer?.cancel();
    super.dispose();
  }

  Future<void> reload() async {
    timer?.cancel();
    try {
      final r = await widget.api.get('/api/downloads${all ? '?all=1' : ''}');
      if (!mounted) return;
      setState(() {
        data = Map<String, dynamic>.from(r);
        error = null;
      });
      if (((r['queued'] ?? 0) as num) + ((r['converting'] ?? 0) as num) > 0) timer = Timer(const Duration(seconds: 2), reload);
    } catch (e) {
      if (mounted) setState(() => error = errorText(e));
    }
  }

  Future<void> run(Future<dynamic> Function() f) async {
    try {
      await f();
    } catch (e) {
      if (mounted) ScaffoldMessenger.of(context).showSnackBar(SnackBar(content: Text(errorText(e))));
    }
    reload();
  }

  String name(Map d) {
    final x = d['item'];
    if (x is! Map || x['title'] == null) return tr('(no longer in the library)');
    return x['type'] == 'episode' ? '${x['grandparentTitle'] ?? ''} · S${x['parentIndex'] ?? 0}E${x['index'] ?? 0} ${x['title']}' : '${x['title']}';
  }

  @override
  Widget build(BuildContext context) {
    final d = data;
    final list = ((d?['downloads'] as List?) ?? const []).map((e) => Map<String, dynamic>.from(e)).toList();
    final busy = ((d?['queued'] ?? 0) as num) + ((d?['converting'] ?? 0) as num);
    final finished = ((d?['ready'] ?? 0) as num) + ((d?['failed'] ?? 0) as num);
    return Scaffold(
      appBar: AppBar(title: Text(tr('Downloads')), actions: [
        if (finished > 0) TextButton(onPressed: () => run(() => widget.api.delete('/api/downloads?state=finished${all ? '&all=1' : ''}')), child: Text(tr('Remove finished ones'))),
      ]),
      body: error != null
          ? Center(child: Text(error!))
          : d == null
              ? const Center(child: CircularProgressIndicator())
              : ListView(padding: const EdgeInsets.all(16), children: [
                  Text(
                    d['allowed'] == true
                        ? tr('With “Download…” on a movie, an episode, a season or a show.{x}', {'x': (d['keep_days'] ?? 0) != 0 ? tr(' Converted copies stay on the server for {d} days.', {'d': d['keep_days']}) : ''})
                        : tr("Downloads are switched off for your account. Ask the server's owner."),
                    style: TextStyle(color: context.faint),
                  ),
                  const SizedBox(height: 8),
                  Text([if (busy > 0) tr('{busy} being converted', {'busy': busy}), tr('{d} ready', {'d': d['ready'] ?? 0}), if (((d['failed'] ?? 0) as num) > 0) tr('{d} failed', {'d': d['failed']})].join(' · ')),
                  if (Poster.admin)
                    CheckboxListTile(
                      dense: true,
                      contentPadding: EdgeInsets.zero,
                      value: all,
                      onChanged: (v) {
                        all = v ?? false;
                        reload();
                      },
                      title: Text(tr("Everyone's downloads")),
                    ),
                  if (list.isEmpty) Padding(padding: EdgeInsets.all(32), child: Center(child: Text(tr('Nothing downloaded yet.')))),
                  for (final x in list) _row(x),
                ]),
    );
  }

  Widget _row(Map<String, dynamic> x) {
    final state = '${x['state']}';
    final parts = ((x['parts'] ?? 1) as num).toInt();
    final size = (x['size'] ?? 0) as num;
    final info = [
      dlQualities['${x['quality']}'] ?? '${x['quality']}',
      if (x['original'] == true && x['quality'] != 'original') tr('sent as it is (already fits)'),
      states[state] ?? state,
      if (state == 'converting') '${x['progress']} %',
      if (size > 0) fmtSize(size),
      if (all && x['user'] != null) '${x['user']}',
    ].join(' · ');
    return Card(
      child: Padding(
        padding: const EdgeInsets.all(10),
        child: Row(children: [
          if (x['thumb'] != null)
            Padding(
              padding: const EdgeInsets.only(right: 10),
              child: Image.network(widget.api.img('${x['thumb']}'), headers: widget.api.auth, width: 40, height: 60, fit: BoxFit.cover, cacheWidth: 100, errorBuilder: (_, _, _) => const SizedBox(width: 40, height: 60)),
            ),
          Expanded(
            child: Column(crossAxisAlignment: CrossAxisAlignment.start, children: [
              Text(name(x), maxLines: 2, overflow: TextOverflow.ellipsis, style: const TextStyle(fontWeight: FontWeight.w600)),
              Text(info, style: TextStyle(color: context.faint, fontSize: 12)),
              if (state == 'converting' || state == 'queued')
                Padding(padding: const EdgeInsets.only(top: 6), child: LinearProgressIndicator(value: ((x['progress'] ?? 0) as num) / 100)),
              if (state == 'failed' && x['error'] != null) Text('${x['error']}', style: TextStyle(color: context.bad, fontSize: 12)),
            ]),
          ),
          if (state == 'ready' && parts > 1)
            for (var n = 0; n < parts; n++) TextButton(onPressed: () => saveFile(context, widget.api, '${x['url']}?part=$n'), child: Text(tr('Part {n}', {'n': n + 1})))
          else if (state == 'ready')
            FilledButton(onPressed: () => saveFile(context, widget.api, '${x['url']}'), child: Text(tr('Save'))),
          if (state == 'failed') TextButton(onPressed: () => run(() => widget.api.post('/api/downloads/${x['id']}/retry')), child: Text(tr('Again'))),
          IconButton(
            tooltip: state == 'converting' ? tr('Stop and remove') : tr('Remove'),
            icon: const Icon(Icons.close),
            onPressed: () => run(() => widget.api.delete('/api/downloads/${x['id']}')),
          ),
        ]),
      ),
    );
  }
}
