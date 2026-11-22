import 'package:flutter/material.dart';
import '../api.dart';
import '../theme.dart';
import '../i18n.dart';

enum _T { text, number, flag, pick, secret, lines }

class _F {
  final String key, label;
  final _T type;
  final String? help;
  final List<(dynamic, String)> options;
  const _F(this.key, this.label, this.type, {this.help, this.options = const []});
}

/// Admin: the server settings (same groups as the web app), driven by a field list.
class ServerSettingsScreen extends StatefulWidget {
  final Api api;
  const ServerSettingsScreen({super.key, required this.api});
  @override
  State<ServerSettingsScreen> createState() => _ServerSettingsScreenState();
}

class _ServerSettingsScreenState extends State<ServerSettingsScreen> {
  Map<String, dynamic> s = {}, resp = {}, tc = {};
  final vals = <String, dynamic>{};
  final ctrls = <String, TextEditingController>{};
  String? error;
  bool loaded = false, saving = false;

  Api get api => widget.api;

  @override
  void initState() {
    super.initState();
    load();
  }

  @override
  void dispose() {
    for (final c in ctrls.values) {
      c.dispose();
    }
    super.dispose();
  }

  Future<void> load() async {
    try {
      final r = await api.get('/api/settings');
      Map<String, dynamic> t = {};
      try {
        t = Map<String, dynamic>.from(await api.get('/api/transcoder') ?? {});
      } catch (_) {}
      if (!mounted) return;
      setState(() {
        resp = Map<String, dynamic>.from(r ?? {});
        s = Map<String, dynamic>.from(resp['settings'] ?? {});
        tc = t;
        vals
          ..clear()
          ..addAll(s);
        for (final c in ctrls.values) {
          c.dispose();
        }
        ctrls.clear();
        loaded = true;
      });
    } catch (e) {
      if (mounted) setState(() => error = '$e');
    }
  }

  List<(dynamic, String)> _lst(String key, dynamic Function(dynamic) v, String Function(dynamic) l) =>
      [for (final e in (resp[key] as List? ?? const [])) (v(e), l(e))];

  List<(dynamic, String)> _nums(List<int> n, String Function(int) l, {bool zero = true, String? zeroLabel}) =>
      [if (zero) (0, zeroLabel ?? tr('Unlimited')), for (final i in n) (i, l(i))];

  List<(dynamic, String)> get _hours => [for (var i = 0; i < 24; i++) (i, '${i.toString().padLeft(2, '0')}:00')];

  Map<String, (String, List<_F>)> get panels {
    final scan = _lst('scan_intervals', (e) => e['minutes'], (e) => '${e['label']}');
    final marks = _lst('credits_marker_choices', (e) => e['value'], (e) => '${e['label']}');
    final playedAt = _lst('played_at_choices', (e) => e['value'], (e) => '${e['label']}');
    final thresholds = _lst('played_thresholds', (e) => e, (e) => '$e %');
    final langs = _lst('languages', (e) => e['code'], (e) => '${e['name']}');
    final presets = _lst('h264_presets', (e) => e, (e) => '$e');
    return {
      'general': (tr('Server'), [
        _F('server_name', tr('Server name'), _T.text),
        _F('port', tr('Port'), _T.number, help: tr('Default 8484. Applies after a restart; the server listens on {resp} now.', {'resp': resp['port_in_use']})),
        _F('public_url', tr('Address in invite links'), _T.text, help: tr('e.g. http://my-nas.example.com:8484. Empty = the address the page was opened with.')),
        _F('invite_default_days', tr('Invite links are valid for (days)'), _T.number),
        _F('home_profiles', tr('Profiles on the sign-in screen'), _T.flag, help: tr('Home profiles can be tapped and opened with a PIN – only on the home network.')),
      ]),
      'network': (tr('Networking'), [
        _F('max_remote_plays', tr('Videos per person outside the home network'), _T.pick, options: _nums([1, 2, 3, 4, 6], (n) => '$n', zeroLabel: tr('Unlimited')), help: tr('Admins are not limited.')),
        _F('paused_stop_minutes', tr('Stop paused playback after'), _T.pick, options: _nums([5, 10, 15, 30, 60, 120], (n) => tr('{n} minutes', {'n': n}), zeroLabel: tr('Never'))),
        _F('remote_max_mbps', tr('Limit streams over the internet'), _T.pick, options: _nums([1, 2, 3, 4, 8, 10, 12, 20], (n) => tr('{n} Mbit/s', {'n': n}))),
        _F('no_auth_networks', tr('Networks without sign-in'), _T.text, help: tr('Addresses or ranges, separated by commas, e.g. 192.168.1.0/24. From there you are signed in as the first admin without a password – only enter networks you trust.')),
        _F('lan_networks', tr('More networks that count as the home network'), _T.text, help: tr('e.g. 100.64.0.0/10 for a VPN.')),
        _F('bind_address', tr('Network card address'), _T.text, help: tr('Empty = all cards. Applies after a restart.')),
        _F('tls_cert', tr('HTTPS certificate (file)'), _T.text, help: tr('Path of the PEM file with its chain. Applies after a restart.')),
        _F('tls_key', tr('HTTPS key (file)'), _T.text, help: tr('Path of the private key (PEM). Empty = plain http://.')),
        _F('http_redirect_port', 'http:// auf https:// umleiten ab Port', _T.number, help: tr('0 = off. Applies after a restart.')),
        _F('relay_server', tr('Relay (address)'), _T.text, help: tr('Reachable from outside without opening a port, e.g. relay.example.com:8485. Empty = off.')),
        _F('relay_secret', tr('Relay (secret)'), _T.secret, help: tr('At least 8 characters.')),
        _F('gdm_enabled', tr('Apps on the home network may find the server'), _T.flag),
        _F('trusted_proxies', tr('Trusted reverse proxies'), _T.text, help: tr('Addresses or ranges, e.g. 127.0.0.1 or 172.16.0.0/12 for Docker.')),
      ]),
      'library': (tr('Library'), [
        _F('group_recent_episodes', tr('Show new episodes on the home page as their show'), _T.flag),
        _F('sort_ignore_articles', tr('Ignore articles when sorting'), _T.flag, help: tr('“The Matrix” is then under M.')),
        _F('sort_articles', tr('Articles to ignore'), _T.text, help: tr('Separated by commas, e.g. the, a, an, der, die, das.')),
        _F('collection_min_items', tr('Collection from … movies'), _T.number, help: tr('2 = every movie series.')),
        _F('scheduled_scans_enabled', tr('Scan the library regularly'), _T.flag),
        _F('scan_interval_minutes', tr('Scan interval'), _T.pick, options: scan),
        _F('watched_threshold_percent', tr('Threshold for “watched”'), _T.pick, options: thresholds),
        _F('watch_folders', tr('Update the library by itself'), _T.flag, help: tr('Folders are checked every 15 seconds.')),
        _F('credits_markers_behaviour', tr('Make credits markers'), _T.pick, options: marks),
        _F('intro_markers_behaviour', tr('Make intro markers'), _T.pick, options: marks),
        _F('credits_from_picture', tr('Detect credits in the picture'), _T.flag, help: tr('Needs ffmpeg.')),
        _F('played_at_behaviour', tr('Mark videos as watched'), _T.pick, options: playedAt),
        _F('continue_watching_weeks', tr('Weeks for “Continue watching”'), _T.number),
        _F('continue_watching_limit', tr('Most items in “Continue watching”'), _T.number),
        _F('continue_watching_include_premieres', tr('Season premieres in “Continue watching”'), _T.flag),
        _F('empty_trash_automatically', tr('Empty the trash after every scan'), _T.flag, help: tr('Off: missing files stay as unavailable until you empty the trash.')),
        _F('allow_media_deletion', tr('Allow deleting media files'), _T.flag),
        _F('download_keep_days', tr('Keep converted downloads (days)'), _T.number, help: tr('0 = until they are removed from the downloads page.')),
      ]),
      'metadata': (tr('Movie and show details'), [
        _F('metadata_enabled', tr('Load plot, cast, posters and episode titles from the internet'), _T.flag),
        _F('meta_cast', tr('Cast and crew'), _T.flag, help: tr('Actors with roles and photos, directors, writers, producers.')),
        _F('cast_photos_limit', tr('Actor photos per movie or show'), _T.pick, options: _nums([10, 20, 40, 100], (n) => '$n', zeroLabel: tr('None')), help: tr('The cast list stays; only this many photos are kept.')),
        _F('meta_person_details', tr('Details about people'), _T.flag, help: tr('Biography, dates of birth, more photos and filmography. Needs a TMDB key; loaded when the page is opened.')),
        _F('meta_artwork', tr('Posters and backgrounds'), _T.flag, help: tr('Off: only pictures from your folders.')),
        _F('meta_episodes', tr('Seasons and episodes'), _T.flag, help: tr('Episode names, plots, air dates and stills.')),
        _F('meta_ratings', tr('Ratings'), _T.flag, help: tr('Ratings from the online source.')),
        _F('metadata_language', tr('Language of titles, plots and episode names'), _T.pick, options: langs, help: tr('A change reloads the details in the background.')),
        _F('imdb_ratings_file', tr("IMDb ratings from IMDb's ratings file"), _T.flag, help: tr('Downloads about 9 MB once a week.')),
        _F('tmdb_api_key', tr('TMDB API key (optional)'), _T.text, help: tr('Without a key: Wikipedia/Wikidata and TVmaze. With a key better matches, actor photos, translated plots.')),
        _F('omdb_api_key', tr('OMDb API key for IMDb plots (optional)'), _T.text),
      ]),
      'extras': ('Extras', [
        _F('trailers_online', tr('Show online trailers'), _T.flag, help: tr('From TMDB or Wikidata, played through YouTube.')),
        _F('trailers_english', tr('Also look for English trailers'), _T.flag),
        _F('trailers_red_band', tr('Allow red band trailers'), _T.flag),
        _F('preroll_mode', tr('Pre-roll: which one plays'), _T.pick, options: [('random', tr('One, at random')), ('all', tr('All, in order')), ('first', tr('Always the first'))]),
        _F('preroll_paths', tr('Pre-roll videos'), _T.lines, help: tr('Played before every movie. One path per line.')),
      ]),
      'tasks': (tr('Scheduled tasks'), [
        _F('maintenance_start_hour', tr('Maintenance starts at'), _T.pick, options: _hours),
        _F('maintenance_end_hour', tr('Maintenance ends at'), _T.pick, options: _hours),
        _F('backup_enabled', tr('Back up the library file every 3 days'), _T.flag),
        _F('backup_dir', tr('Backup folder'), _T.text, help: tr('Empty = the folder “backups” in the data folder.')),
        _F('backups_keep', tr('Backups to keep'), _T.number),
        _F('db_cache_mb', tr('Database cache (MB)'), _T.number),
        _F('optimize_weekly', tr('Optimize the library file every week'), _T.flag),
        _F('cleanup_weekly', tr('Remove old cache files every week'), _T.flag),
        _F('analyze_loudness', tr('Measure the loudness of the videos'), _T.flag, help: tr('Needs ffmpeg.')),
        _F('deep_analysis', tr('Deep analysis'), _T.flag, help: tr('Reads every file in full – the disks stay busy for a long time. Needs ffprobe.')),
        _F('generate_thumbnails', tr('Make preview pictures'), _T.flag, help: tr('Needs ffmpeg.')),
        _F('theme_from_intros', tr('Theme music from show intros'), _T.flag, help: tr('Once per show in the maintenance window. A theme.mp3 in the folder comes first. Needs ffmpeg.')),
        _F('refresh_metadata', tr('Read all libraries again every 3 days'), _T.flag),
      ]),
      'transcoder': (tr('Transcoder'), [
        _F('transcoder_quality', tr('Transcoder quality'), _T.pick, options: [('automatic', tr('Automatic')), ('speed', tr('Faster')), ('quality', tr('Better quality')), ('max', tr('Maximum (the processor glows)'))]),
        _F('boost_dialogue', tr('Raise the dialogue when mixing down to stereo'), _T.flag),
        _F('transcoding_enabled', tr('Allow converting videos'), _T.flag, help: tr('Off: only files the player can play directly are played.')),
        _F('transcode_dir', tr('Folder for live conversions'), _T.text, help: tr('Empty = the folder “transcode” in the data folder.')),
        _F('downloads_dir', tr('Folder for converted downloads'), _T.text),
        _F('hw_accel', tr('Hardware acceleration'), _T.pick, options: [('auto', tr('Automatic')), ('none', tr('Off (software)')), ('vaapi', 'Intel/AMD VAAPI'), ('qsv', 'Intel Quick Sync'), ('nvenc', 'NVIDIA')]),
        _F('hw_device', tr('Graphics device'), _T.text, help: tr('Empty = the first one. VAAPI: a device file like /dev/dri/renderD129. NVIDIA: the card number.')),
        _F('hdr_tone_mapping', tr('Convert HDR to normal colours'), _T.flag),
        _F('hw_constant_quality', tr('Graphics chip: convert by quality instead of bitrate'), _T.flag),
        _F('h264_preset', tr('Speed of the software encoder (x264)'), _T.pick, options: presets),
        _F('h264_minimum_crf', tr('Best x264 quality allowed (CRF)'), _T.number),
        _F('max_transcodes', tr('Transcodes at the same time (0 = no limit)'), _T.number),
        _F('max_gpu_transcodes', tr('… of them on the graphics chip'), _T.number),
        _F('max_cpu_transcodes', tr('… of them on the processor'), _T.number),
        _F('hls_segment_seconds', tr('HLS segment length (seconds)'), _T.number),
        _F('segmented_transcoder_timeout_seconds', tr('Timeout of the segmenting transcoder (seconds)'), _T.number),
        _F('ffmpeg', tr('ffmpeg program'), _T.text),
        _F('ffprobe', tr('ffprobe program'), _T.text),
      ]),
    };
  }

  Future<void> save() async {
    final body = <String, dynamic>{'utc_offset_minutes': -DateTime.now().timeZoneOffset.inMinutes};
    for (final entry in panels.values) {
      for (final f in entry.$2) {
        final v = vals[f.key];
        final old = s[f.key];
        if ('$v' == '$old') continue;
        body[f.key] = v;
      }
    }
    setState(() => saving = true);
    try {
      final x = await api.put('/api/settings', body);
      final msg = [
        tr('Saved'),
        if (x is Map && x['refetching'] != null && x['refetching'] != 0) tr('Details for {x} titles are being reloaded', {'x': x['refetching']}),
        if (x is Map && x['restart_needed'] != null) '${x['restart_needed']}',
      ].join(' · ');
      if (mounted) ScaffoldMessenger.of(context).showSnackBar(SnackBar(content: Text(msg)));
      await load();
    } catch (e) {
      if (mounted) ScaffoldMessenger.of(context).showSnackBar(SnackBar(content: Text('$e')));
    } finally {
      if (mounted) setState(() => saving = false);
    }
  }

  Widget field(_F f) {
    final help = f.help == null ? null : Text(f.help!, style: TextStyle(fontSize: 12, color: context.faint));
    Widget w;
    switch (f.type) {
      case _T.flag:
        w = CheckboxListTile(
          contentPadding: EdgeInsets.zero,
          controlAffinity: ListTileControlAffinity.leading,
          title: Text(f.label),
          subtitle: help,
          value: vals[f.key] == true,
          onChanged: (v) => setState(() => vals[f.key] = v == true),
        );
      case _T.pick:
        final opts = f.options.toList();
        final cur = vals[f.key];
        if (!opts.any((o) => '${o.$1}' == '$cur')) opts.add((cur, '$cur'));
        w = DropdownButtonFormField<String>(
          initialValue: '$cur',
          isExpanded: true,
          decoration: InputDecoration(labelText: f.label, helperText: f.help, helperMaxLines: 4),
          items: [for (final o in opts) DropdownMenuItem(value: '${o.$1}', child: Text(o.$2, overflow: TextOverflow.ellipsis))],
          onChanged: (v) => setState(() => vals[f.key] = opts.firstWhere((o) => '${o.$1}' == v).$1),
        );
      default:
        final c = ctrls.putIfAbsent(f.key, () => TextEditingController(text: '${vals[f.key] ?? ''}'));
        w = TextField(
          controller: c,
          obscureText: f.type == _T.secret,
          keyboardType: f.type == _T.number ? TextInputType.number : null,
          minLines: f.type == _T.lines ? 3 : 1,
          maxLines: f.type == _T.lines ? 6 : 1,
          decoration: InputDecoration(labelText: f.label, helperText: f.help, helperMaxLines: 4, border: f.type == _T.lines ? const OutlineInputBorder() : null),
          onChanged: (v) => vals[f.key] = f.type == _T.number ? (num.tryParse(v.trim()) ?? v) : v,
        );
    }
    return Padding(padding: const EdgeInsets.symmetric(vertical: 6), child: w);
  }

  Widget info() {
    if (tc.isEmpty) return const SizedBox.shrink();
    final found = tc['ffmpeg_found'] == true;
    final hw = (tc['hardware_encoders'] as List?) ?? const [];
    final work = (tc['hardware_working'] as List?) ?? const [];
    return Padding(
      padding: const EdgeInsets.only(bottom: 8),
      child: Column(crossAxisAlignment: CrossAxisAlignment.start, children: [
        Text(found ? tr('ffmpeg found: {tc}', {'tc': tc['ffmpeg_version']}) : tr('ffmpeg was not found. Install it or give its path below; without ffmpeg only browser-friendly files play.'),
            style: TextStyle(color: found ? context.good : context.bad)),
        if (hw.isNotEmpty) Text(tr('Hardware encoders: {hw}', {'hw': hw.join(', ')}), style: TextStyle(color: context.dim)),
        if (hw.isNotEmpty) Text(work.isNotEmpty ? tr('Works here: {work}', {'work': work.join(', ')}) : tr('None of them works here, the processor converts.'), style: TextStyle(color: context.dim)),
        Text(tr('Converting with: {tc}', {'tc': tc['hw_in_use'] == 'none' ? tr('the processor (software)') : tc['hw_in_use']}), style: TextStyle(color: context.dim)),
      ]),
    );
  }

  @override
  Widget build(BuildContext context) {
    if (error != null) return Scaffold(appBar: AppBar(title: Text(tr('Server settings'))), body: Center(child: Text(error!)));
    if (!loaded) return Scaffold(appBar: AppBar(title: Text(tr('Server settings'))), body: const Center(child: CircularProgressIndicator()));
    final ps = panels;
    return DefaultTabController(
      length: ps.length,
      child: Scaffold(
        appBar: AppBar(
          title: Text(tr('Server settings')),
          actions: [
            Padding(
              padding: const EdgeInsets.only(right: 12),
              child: FilledButton.icon(onPressed: saving ? null : save, icon: const Icon(Icons.save_outlined), label: Text(tr('Save'))),
            ),
          ],
          bottom: TabBar(isScrollable: true, tabs: [for (final p in ps.values) Tab(text: p.$1)]),
        ),
        body: TabBarView(children: [
          for (final e in ps.entries)
            ListView(padding: const EdgeInsets.all(16), children: [
              if (e.key == 'transcoder') info(),
              if (e.key == 'general') Padding(padding: const EdgeInsets.only(bottom: 8), child: Text(tr('Data folder on the server: {resp}', {'resp': resp['data_dir']}), style: TextStyle(color: context.dim))),
              if (e.key == 'network' && resp['lan_url'] != null) Padding(padding: const EdgeInsets.only(bottom: 8), child: Text(tr('Address on the home network: {resp}', {'resp': resp['lan_url']}), style: TextStyle(color: context.dim))),
              for (final f in e.value.$2) field(f),
            ]),
        ]),
      ),
    );
  }
}
