import 'package:flutter/material.dart';
import '../api.dart';
import '../theme.dart';

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

  List<(dynamic, String)> _nums(List<int> n, String Function(int) l, {bool zero = true, String zeroLabel = 'Unbegrenzt'}) =>
      [if (zero) (0, zeroLabel), for (final i in n) (i, l(i))];

  List<(dynamic, String)> get _hours => [for (var i = 0; i < 24; i++) (i, '${i.toString().padLeft(2, '0')}:00')];

  Map<String, (String, List<_F>)> get panels {
    final scan = _lst('scan_intervals', (e) => e['minutes'], (e) => '${e['label']}');
    final marks = _lst('credits_marker_choices', (e) => e['value'], (e) => '${e['label']}');
    final playedAt = _lst('played_at_choices', (e) => e['value'], (e) => '${e['label']}');
    final thresholds = _lst('played_thresholds', (e) => e, (e) => '$e %');
    final langs = _lst('languages', (e) => e['code'], (e) => '${e['name']}');
    final presets = _lst('h264_presets', (e) => e, (e) => '$e');
    return {
      'general': ('Server', [
        const _F('server_name', 'Servername', _T.text),
        _F('port', 'Port', _T.number, help: 'Standard 8484. Gilt nach einem Neustart; aktuell lauscht der Server auf ${resp['port_in_use']}.'),
        const _F('public_url', 'Adresse in Einladungslinks', _T.text, help: 'z. B. http://mein-nas.example.com:8484. Leer = Adresse, mit der die Seite geöffnet wurde.'),
        const _F('invite_default_days', 'Einladungslinks gelten (Tage)', _T.number),
        const _F('home_profiles', 'Profile auf dem Anmeldebildschirm', _T.flag, help: 'Heimprofile lassen sich mit PIN antippen – nur im Heimnetz.'),
      ]),
      'network': ('Netzwerk', [
        _F('max_remote_plays', 'Videos pro Person außerhalb des Heimnetzes', _T.pick, options: _nums([1, 2, 3, 4, 6], (n) => '$n', zeroLabel: 'Unbegrenzt'), help: 'Admins sind nicht begrenzt.'),
        _F('paused_stop_minutes', 'Pausierte Wiedergabe beenden nach', _T.pick, options: _nums([5, 10, 15, 30, 60, 120], (n) => '$n Minuten', zeroLabel: 'Nie')),
        _F('remote_max_mbps', 'Streams übers Internet begrenzen', _T.pick, options: _nums([1, 2, 3, 4, 8, 10, 12, 20], (n) => '$n Mbit/s')),
        const _F('no_auth_networks', 'Netze ohne Anmeldung', _T.text, help: 'Adressen oder Bereiche, durch Komma getrennt, z. B. 192.168.1.0/24. Von dort ist man ohne Passwort als erster Admin angemeldet – nur vertrauenswürdige Netze eintragen.'),
        const _F('lan_networks', 'Weitere Netze, die als Heimnetz zählen', _T.text, help: 'z. B. 100.64.0.0/10 für ein VPN.'),
        const _F('bind_address', 'Adresse der Netzwerkkarte', _T.text, help: 'Leer = alle Karten. Gilt nach einem Neustart.'),
        const _F('tls_cert', 'HTTPS-Zertifikat (Datei)', _T.text, help: 'Pfad der PEM-Datei samt Kette. Gilt nach einem Neustart.'),
        const _F('tls_key', 'HTTPS-Schlüssel (Datei)', _T.text, help: 'Pfad des privaten Schlüssels (PEM). Leer = einfaches http://.'),
        const _F('http_redirect_port', 'http:// auf https:// umleiten ab Port', _T.number, help: '0 = aus. Gilt nach einem Neustart.'),
        const _F('relay_server', 'Relay (Adresse)', _T.text, help: 'Erreichbarkeit von außen ohne Portfreigabe, z. B. relay.example.com:8485. Leer = aus.'),
        const _F('relay_secret', 'Relay (Geheimnis)', _T.secret, help: 'Mindestens 8 Zeichen.'),
        const _F('gdm_enabled', 'Apps im Heimnetz dürfen den Server finden', _T.flag),
        const _F('trusted_proxies', 'Vertrauenswürdige Reverse-Proxys', _T.text, help: 'Adressen oder Bereiche, z. B. 127.0.0.1 oder 172.16.0.0/12 für Docker.'),
      ]),
      'library': ('Bibliothek', [
        const _F('group_recent_episodes', 'Neue Folgen auf der Startseite als Serie zeigen', _T.flag),
        const _F('sort_ignore_articles', 'Artikel beim Sortieren ignorieren', _T.flag, help: '„The Matrix“ steht dann unter M.'),
        const _F('sort_articles', 'Zu ignorierende Artikel', _T.text, help: 'Durch Komma getrennt, z. B. the, a, an, der, die, das.'),
        const _F('collection_min_items', 'Sammlung ab … Filmen', _T.number, help: '2 = jede Filmreihe.'),
        const _F('scheduled_scans_enabled', 'Bibliothek regelmäßig scannen', _T.flag),
        _F('scan_interval_minutes', 'Scan-Intervall', _T.pick, options: scan),
        _F('watched_threshold_percent', 'Schwelle für „gesehen“', _T.pick, options: thresholds),
        const _F('watch_folders', 'Bibliothek automatisch aktualisieren', _T.flag, help: 'Ordner werden alle 15 Sekunden geprüft.'),
        _F('credits_markers_behaviour', 'Abspann-Markierungen erzeugen', _T.pick, options: marks),
        _F('intro_markers_behaviour', 'Intro-Markierungen erzeugen', _T.pick, options: marks),
        const _F('credits_from_picture', 'Abspann am Bild erkennen', _T.flag, help: 'Braucht ffmpeg.'),
        _F('played_at_behaviour', 'Videos als gesehen markieren', _T.pick, options: playedAt),
        const _F('continue_watching_weeks', 'Wochen für „Weiterschauen“', _T.number),
        const _F('continue_watching_limit', 'Höchstzahl bei „Weiterschauen“', _T.number),
        const _F('continue_watching_include_premieres', 'Staffelpremieren in „Weiterschauen“', _T.flag),
        const _F('empty_trash_automatically', 'Papierkorb nach jedem Scan leeren', _T.flag, help: 'Aus: fehlende Dateien bleiben als nicht verfügbar, bis du den Papierkorb leerst.'),
        const _F('allow_media_deletion', 'Löschen von Mediendateien erlauben', _T.flag),
        const _F('download_keep_days', 'Konvertierte Downloads behalten (Tage)', _T.number, help: '0 = bis sie von der Download-Seite entfernt werden.'),
      ]),
      'metadata': ('Film- und Seriendetails', [
        const _F('metadata_enabled', 'Handlung, Besetzung, Poster und Folgentitel aus dem Internet laden', _T.flag),
        const _F('meta_cast', 'Besetzung und Team', _T.flag, help: 'Schauspieler mit Rollen und Fotos, Regie, Drehbuch, Produktion.'),
        _F('cast_photos_limit', 'Schauspielerfotos pro Film oder Serie', _T.pick, options: _nums([10, 20, 40, 100], (n) => '$n', zeroLabel: 'Keine'), help: 'Die Besetzungsliste bleibt; nur so viele Fotos werden gespeichert.'),
        const _F('meta_person_details', 'Details zu Personen', _T.flag, help: 'Biografie, Geburtsdaten, weitere Fotos und Filmografie. Braucht einen TMDB-Schlüssel; wird beim Öffnen der Seite geladen.'),
        const _F('meta_artwork', 'Poster und Hintergründe', _T.flag, help: 'Aus: nur Bilder aus deinen Ordnern.'),
        const _F('meta_episodes', 'Staffeln und Folgen', _T.flag, help: 'Folgennamen, Handlung, Sendedatum und Standbilder.'),
        const _F('meta_ratings', 'Bewertungen', _T.flag, help: 'Bewertungen der Online-Quelle.'),
        _F('metadata_language', 'Sprache von Titeln, Handlungen und Folgennamen', _T.pick, options: langs, help: 'Eine Änderung lädt die Details im Hintergrund neu.'),
        const _F('imdb_ratings_file', 'IMDb-Bewertungen aus IMDbs Bewertungsdatei', _T.flag, help: 'Lädt einmal pro Woche ca. 9 MB.'),
        const _F('tmdb_api_key', 'TMDB-API-Schlüssel (optional)', _T.text, help: 'Ohne Schlüssel: Wikipedia/Wikidata und TVmaze. Mit Schlüssel bessere Treffer, Darstellerfotos, übersetzte Handlungen.'),
        const _F('omdb_api_key', 'OMDb-API-Schlüssel für IMDb-Handlungen (optional)', _T.text),
      ]),
      'extras': ('Extras', [
        const _F('trailers_online', 'Online-Trailer zeigen', _T.flag, help: 'Von TMDB oder Wikidata, abgespielt über YouTube.'),
        const _F('trailers_english', 'Auch englische Trailer suchen', _T.flag),
        const _F('trailers_red_band', 'Red-Band-Trailer erlauben', _T.flag),
        const _F('preroll_mode', 'Vorspann: welcher wird gespielt', _T.pick, options: [('random', 'Einer, zufällig'), ('all', 'Alle in der Reihenfolge'), ('first', 'Immer der erste')]),
        const _F('preroll_paths', 'Vorspann-Videos', _T.lines, help: 'Vor jedem Film gespielt. Ein Pfad pro Zeile.'),
      ]),
      'tasks': ('Geplante Aufgaben', [
        _F('maintenance_start_hour', 'Wartung beginnt um', _T.pick, options: _hours),
        _F('maintenance_end_hour', 'Wartung endet um', _T.pick, options: _hours),
        const _F('backup_enabled', 'Bibliotheksdatei alle 3 Tage sichern', _T.flag),
        const _F('backup_dir', 'Sicherungsordner', _T.text, help: 'Leer = Ordner „backups“ im Datenordner.'),
        const _F('backups_keep', 'Zu behaltende Sicherungen', _T.number),
        const _F('db_cache_mb', 'Datenbank-Cache (MB)', _T.number),
        const _F('optimize_weekly', 'Bibliotheksdatei wöchentlich optimieren', _T.flag),
        const _F('cleanup_weekly', 'Alte Cache-Dateien wöchentlich entfernen', _T.flag),
        const _F('analyze_loudness', 'Lautheit der Videos messen', _T.flag, help: 'Braucht ffmpeg.'),
        const _F('deep_analysis', 'Tiefenanalyse', _T.flag, help: 'Liest jede Datei komplett – Platten bleiben lange beschäftigt. Braucht ffprobe.'),
        const _F('generate_thumbnails', 'Vorschaubilder erzeugen', _T.flag, help: 'Braucht ffmpeg.'),
        const _F('refresh_metadata', 'Alle Bibliotheken alle 3 Tage neu lesen', _T.flag),
      ]),
      'transcoder': ('Transcoder', [
        _F('transcoder_quality', 'Transcoder-Qualität', _T.pick, options: const [('automatic', 'Automatisch'), ('speed', 'Höhere Geschwindigkeit'), ('quality', 'Höhere Qualität'), ('max', 'Maximal (CPU glüht)')]),
        const _F('boost_dialogue', 'Dialoge beim Herunterrechnen auf Stereo anheben', _T.flag),
        const _F('transcoding_enabled', 'Videokonvertierung erlauben', _T.flag, help: 'Aus: Es spielen nur Dateien, die der Player direkt abspielen kann.'),
        const _F('transcode_dir', 'Ordner für Live-Konvertierungen', _T.text, help: 'Leer = Ordner „transcode“ im Datenordner.'),
        const _F('downloads_dir', 'Ordner für konvertierte Downloads', _T.text),
        _F('hw_accel', 'Hardwarebeschleunigung', _T.pick, options: const [('auto', 'Automatisch'), ('none', 'Aus (Software)'), ('vaapi', 'Intel/AMD VAAPI'), ('qsv', 'Intel Quick Sync'), ('nvenc', 'NVIDIA')]),
        const _F('hw_device', 'Grafikgerät', _T.text, help: 'Leer = das erste. VAAPI: Gerätedatei wie /dev/dri/renderD129. NVIDIA: Kartennummer.'),
        const _F('hdr_tone_mapping', 'HDR in normale Farben umrechnen', _T.flag),
        const _F('hw_constant_quality', 'Grafikchip: nach Qualität statt Bitrate', _T.flag),
        _F('h264_preset', 'Geschwindigkeit des Software-Encoders (x264)', _T.pick, options: presets),
        const _F('h264_minimum_crf', 'Beste erlaubte x264-Qualität (CRF)', _T.number),
        const _F('max_transcodes', 'Gleichzeitige Transcodes (0 = unbegrenzt)', _T.number),
        const _F('max_gpu_transcodes', '… davon mit Grafikchip', _T.number),
        const _F('max_cpu_transcodes', '… davon mit Prozessor', _T.number),
        const _F('hls_segment_seconds', 'HLS-Segmentlänge (Sekunden)', _T.number),
        const _F('segmented_transcoder_timeout_seconds', 'Timeout des segmentierten Transcoders (Sekunden)', _T.number),
        const _F('ffmpeg', 'ffmpeg-Programm', _T.text),
        const _F('ffprobe', 'ffprobe-Programm', _T.text),
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
        'Gespeichert',
        if (x is Map && x['refetching'] != null && x['refetching'] != 0) 'Details für ${x['refetching']} Titel werden neu geladen',
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
        Text(found ? 'ffmpeg gefunden: ${tc['ffmpeg_version']}' : 'ffmpeg wurde nicht gefunden. Installiere es oder gib unten den Pfad an; ohne ffmpeg spielen nur browserfreundliche Dateien.',
            style: TextStyle(color: found ? context.good : context.bad)),
        if (hw.isNotEmpty) Text('Hardware-Encoder: ${hw.join(', ')}', style: TextStyle(color: context.dim)),
        if (hw.isNotEmpty) Text(work.isNotEmpty ? 'Funktioniert hier: ${work.join(', ')}' : 'Keiner davon funktioniert hier, der Prozessor konvertiert.', style: TextStyle(color: context.dim)),
        Text('Konvertiert mit: ${tc['hw_in_use'] == 'none' ? 'dem Prozessor (Software)' : tc['hw_in_use']}', style: TextStyle(color: context.dim)),
      ]),
    );
  }

  @override
  Widget build(BuildContext context) {
    if (error != null) return Scaffold(appBar: AppBar(title: const Text('Servereinstellungen')), body: Center(child: Text(error!)));
    if (!loaded) return Scaffold(appBar: AppBar(title: const Text('Servereinstellungen')), body: const Center(child: CircularProgressIndicator()));
    final ps = panels;
    return DefaultTabController(
      length: ps.length,
      child: Scaffold(
        appBar: AppBar(
          title: const Text('Servereinstellungen'),
          actions: [
            Padding(
              padding: const EdgeInsets.only(right: 12),
              child: FilledButton.icon(onPressed: saving ? null : save, icon: const Icon(Icons.save_outlined), label: const Text('Speichern')),
            ),
          ],
          bottom: TabBar(isScrollable: true, tabs: [for (final p in ps.values) Tab(text: p.$1)]),
        ),
        body: TabBarView(children: [
          for (final e in ps.entries)
            ListView(padding: const EdgeInsets.all(16), children: [
              if (e.key == 'transcoder') info(),
              if (e.key == 'general') Padding(padding: const EdgeInsets.only(bottom: 8), child: Text('Datenordner auf dem Server: ${resp['data_dir']}', style: TextStyle(color: context.dim))),
              if (e.key == 'network' && resp['lan_url'] != null) Padding(padding: const EdgeInsets.only(bottom: 8), child: Text('Adresse im Heimnetz: ${resp['lan_url']}', style: TextStyle(color: context.dim))),
              for (final f in e.value.$2) field(f),
            ]),
        ]),
      ),
    );
  }
}
