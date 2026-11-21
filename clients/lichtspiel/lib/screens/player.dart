import 'dart:async';
import 'package:flutter/foundation.dart';
import 'package:flutter/material.dart';
import 'package:flutter/services.dart';
import 'package:media_kit/media_kit.dart';
import 'package:media_kit_video/media_kit_video.dart';
import 'package:shared_preferences/shared_preferences.dart';
import '../api.dart';
import '../clock.dart';
import '../widgets/poster.dart';

const _speeds = [0.5, 0.75, 1.0, 1.25, 1.5, 1.75, 2.0];
/// The server's names (QUALITIES in src/transcode.rs); also offered in Settings.
const playerQualities = {
  'original': 'Originalqualität',
  '1080p-20': '1080p 20 Mbit/s',
  '1080p-12': '1080p 12 Mbit/s',
  '1080p-8': '1080p 8 Mbit/s',
  '720p-4': '720p 4 Mbit/s',
  '720p-3': '720p 3 Mbit/s',
  '720p-2': '720p 2 Mbit/s',
  '480p-1.5': '480p 1,5 Mbit/s',
  '320p-0.7': '320p 0,7 Mbit/s',
};
const playerSubSizes = {75: 'Klein', 100: 'Normal', 130: 'Groß', 170: 'Sehr groß'};

/// Older versions saved 2160/1080/720/480/360, which the server does not know (it then converts at full size).
String qualityKey(String? q) =>
    playerQualities.containsKey(q) ? q! : const {'1080': '1080p-20', '720': '720p-4', '480': '480p-1.5', '360': '320p-0.7'}[q] ?? 'original';

/// Older versions offered 125/150/200 %: use the nearest size offered now.
int subSizeKey(int? v) => v == null ? 100 : playerSubSizes.keys.reduce((a, b) => (b - v).abs() < (a - v).abs() ? b : a);

const _stillWatchingAfter = 3;

/// mpv reads a negative seek as "counted from the end", so going back stops at the start.
Duration back10(Duration p) => p < const Duration(seconds: 10) ? Duration.zero : p - const Duration(seconds: 10);

/// Keeps a player choice on this device and in the account.
Future<void> savePlayerPref(Api api, String k, Object v) async {
  final p = await SharedPreferences.getInstance();
  if (v is String) await p.setString(k, v);
  if (v is double) await p.setDouble(k, v);
  if (v is int) await p.setInt(k, v);
  if (v is bool) await p.setBool(k, v);
  // The account keeps it too (like the web player); Settings copies the account's values over this device's.
  try {
    await api.post('/api/me/prefs', {k: v is bool ? (v ? '1' : '0') : '$v'});
  } catch (_) {}
}

/// Plays through libmpv, which handles mkv/HEVC/HDR/ASS directly, so the server's direct URL is used.
/// With a quality below "Original" the server converts (HLS). Settings like the web player: quality, speed,
/// audio track, subtitles and their size, chapters, skip intro, next episode by itself, "still watching?".
class PlayerScreen extends StatefulWidget {
  /// "Zur Warteschlange": plays before anything else that would follow (kept while the app runs).
  static final List<Map<String, dynamic>> userQueue = [];
  /// The sleep timer, kept over the episodes that follow until it fires or the player closes:
  /// pause at [sleepAt], or stop at the end of the title ([sleepEnd]).
  static DateTime? sleepAt;
  static bool sleepEnd = false;
  final Api api;
  final Map<String, dynamic> item;
  final List<Map<String, dynamic>> queue; // what follows (a playlist, the rest of a season, ...)
  final int autoRun; // episodes in a row that played by themselves
  final String? together; // the code of a "Zusammen schauen" room: play, pause and jumps are shared
  const PlayerScreen({super.key, required this.api, required this.item, this.queue = const [], this.autoRun = 0, this.together});
  @override
  State<PlayerScreen> createState() => _PlayerScreenState();
}

const _iso3 = {'en': 'eng', 'de': 'deu', 'fr': 'fra', 'es': 'spa', 'it': 'ita', 'pt': 'por', 'nl': 'nld', 'sv': 'swe', 'da': 'dan', 'no': 'nor', 'fi': 'fin', 'pl': 'pol', 'ru': 'rus', 'tr': 'tur', 'ja': 'jpn', 'ko': 'kor', 'zh': 'zho'};

/// A track language (two or three letters, bibliographic or terminologic) against a two-letter preference.
bool _sameLang(String? track, String want) {
  final t = (track ?? '').toLowerCase();
  if (t.isEmpty) return false;
  final w = want.toLowerCase();
  return t == w || t == _iso3[w] || (w == 'de' && t == 'ger') || (w == 'fr' && t == 'fre') || (w == 'zh' && t == 'chi') || (w == 'nl' && t == 'dut');
}

class _PlayerScreenState extends State<PlayerScreen> {
  final player = Player();
  late final VideoController controller = VideoController(player);
  final subs = <StreamSubscription>[];
  Timer? ticker, countdown, sleepTimer;
  double? sleepVol; // the volume before the sleep timer began to fade it out
  bool goingOn = false; // the next title replaces this player (the sleep timer stays)

  Map<String, dynamic> it = {};
  String? error;
  bool triedHls = false;
  bool checkingError = false; // an error came while playing; waiting to see whether playback goes on
  String get id => '${widget.item['ratingKey'] ?? widget.item['id']}';

  // Preferences (kept between videos).
  String quality = 'original';
  double speed = 1;
  int subSize = 100;
  bool autoNext = true;
  String subLang = '';
  String audioLang = '';
  int? audioIdx; // the file's audio track asked from the server (when the picture is converted)
  bool converted = false; // the server sends the picture converted: the app sees one audio track only
  bool restoredAudio = false;
  String subTicket = ''; // lets the subtitle tracks load: libmpv sends no sign-in header for them
  SubtitleTrack? pickedSub; // what was chosen here (the player's own state can lag behind or say "auto")
  // The chapter marks on the seek bar show like the bar itself: after the mouse moved, for three seconds.
  final ticksOn = ValueNotifier<bool>(false);
  Timer? ticksTimer;
  final hoverPos = ValueNotifier<Offset?>(null); // the mouse over the video (for the seek bar's time, chapter and still)

  void mouseMoved() {
    // Somebody is there: the sleep timer that is fading the sound out is switched off.
    if (sleepVol != null && PlayerScreen.sleepAt != null) {
      setSleep(null);
      tell('Sleep-Timer aus');
    }
    if (!ticksOn.value) ticksOn.value = true;
    ticksTimer?.cancel();
    ticksTimer = Timer(const Duration(seconds: 3), () => ticksOn.value = false);
  }

  /// Over the seek bar: the time, the chapter's name and a still from the server, like the web app.
  Widget seekTip() {
    return Positioned.fill(
      child: IgnorePointer(
        child: ValueListenableBuilder<Offset?>(
          valueListenable: hoverPos,
          builder: (_, pos, __) => LayoutBuilder(builder: (_, box) {
            final total = player.state.duration.inMilliseconds;
            if (pos == null || total <= 0) return const SizedBox();
            final fromBottom = box.maxHeight - pos.dy;
            if (fromBottom < 52 || fromBottom > 96 || pos.dx < 16 || pos.dx > box.maxWidth - 16) return const SizedBox();
            final ms = ((pos.dx - 16) / (box.maxWidth - 32) * total).round().clamp(0, total);
            Map<String, dynamic>? chap;
            if (chapters.length > 1) {
              for (final c in chapters) {
                if (((c['start'] ?? 0) as num) <= ms) chap = c;
              }
            }
            final name = chap == null ? '' : ' · ${chap['title']}'.replaceFirstMapped(RegExp(r'Chapter (\d+)$'), (m) => 'Kapitel ${m[1]}');
            const w = 208.0;
            final left = (pos.dx - w / 2).clamp(8.0, box.maxWidth - w - 8);
            final step = ms ~/ 10000 * 10;
            return Stack(children: [
              Positioned(
                left: left,
                bottom: 100,
                width: w,
                child: DecoratedBox(
                  decoration: BoxDecoration(color: Colors.black87, borderRadius: BorderRadius.circular(6)),
                  child: Padding(
                    padding: const EdgeInsets.all(4),
                    child: Column(mainAxisSize: MainAxisSize.min, children: [
                      if (it['Media'] != null)
                        AspectRatio(
                          aspectRatio: 16 / 9,
                          child: Image.network(widget.api.abs('/library/parts/$id/frame?t=$step'), headers: widget.api.auth, fit: BoxFit.cover, gaplessPlayback: true, errorBuilder: (_, __, ___) => const SizedBox()),
                        ),
                      const SizedBox(height: 4),
                      Text('${fmtClock(ms)}$name', textAlign: TextAlign.center, maxLines: 2, overflow: TextOverflow.ellipsis, style: const TextStyle(color: Colors.white, fontSize: 12)),
                    ]),
                  ),
                ),
              ),
            ]);
          }),
        ),
      ),
    );
  }

  /// Thin marks on the seek bar at the start of each chapter (the first one is the start).
  Widget chapterTicks() {
    return Positioned(
      left: 16,
      right: 16,
      bottom: 56 + 18 - 7,
      height: 14,
      child: IgnorePointer(
        child: ValueListenableBuilder<bool>(
          valueListenable: ticksOn,
          builder: (_, on, __) => AnimatedOpacity(
            opacity: on ? 1 : 0,
            duration: const Duration(milliseconds: 150),
            child: StreamBuilder<Duration>(
              stream: player.stream.duration,
              builder: (_, snap) {
                final total = (snap.data ?? player.state.duration).inMilliseconds;
                if (total <= 0 || chapters.length < 2) return const SizedBox();
                return LayoutBuilder(
                  builder: (_, box) => Stack(children: [
                    for (final c in chapters.skip(1))
                      Positioned(
                        left: (box.maxWidth * (((c['start'] ?? 0) as num) / total).clamp(0.0, 1.0)) - 1,
                        top: 0,
                        bottom: 0,
                        width: 3,
                        child: const DecoratedBox(decoration: BoxDecoration(color: Colors.white, boxShadow: [BoxShadow(color: Colors.black87, blurRadius: 2)])),
                      ),
                  ]),
                );
              },
            ),
          ),
        ),
      ),
    );
  }

  List<Map<String, dynamic>> externalSubs = []; // subtitle files next to the video
  List<Map<String, dynamic>> chapters = [];

  // Settings -> Extras -> pre-roll: a short video before a movie that starts from the beginning.
  Completer<bool>? prerollEnd; // completes with true to skip all of them
  bool prerollSeveral = false;

  Future<void> playPreroll() async {
    List urls;
    try {
      final r = await widget.api.get('/api/preroll');
      urls = (r is Map ? r['urls'] as List? : null) ?? const [];
    } catch (_) {
      return;
    }
    prerollSeveral = urls.length > 1;
    for (final u in urls) {
      if (!mounted) return;
      final end = Completer<bool>();
      prerollEnd = end;
      setState(() {});
      final done = player.stream.completed.listen((c) {
        if (c && !end.isCompleted) end.complete(false);
      });
      final err = player.stream.error.listen((e) {
        if (e.isNotEmpty && !end.isCompleted) end.complete(false);
      });
      try {
        await player.open(Media(widget.api.abs('$u'), httpHeaders: widget.api.auth));
        if (await end.future) break;
      } catch (_) {
        // a pre-roll that does not play is skipped
      } finally {
        await done.cancel();
        await err.cancel();
      }
    }
    prerollEnd = null;
    if (mounted) setState(() {});
  }

  Widget prerollButtons() {
    final end = prerollEnd;
    if (end == null) return const SizedBox.shrink();
    return Positioned(
      right: 24,
      bottom: 110,
      child: Column(crossAxisAlignment: CrossAxisAlignment.end, children: [
        if (prerollSeveral) FilledButton.tonal(onPressed: () => end.isCompleted ? null : end.complete(false), child: const Text('Nächstes Video')),
        const SizedBox(height: 8),
        FilledButton(onPressed: () => end.isCompleted ? null : end.complete(true), child: Text(prerollSeveral ? 'Alle überspringen' : 'Überspringen')),
      ]),
    );
  }

  // The words of a song: lines that follow the music (.lrc, with times) or plain text.
  List<Map<String, dynamic>> lyrics = [];
  bool lyricsSynced = false, lyricsOn = true;
  final lyricScroll = ScrollController();

  void loadLyrics() {
    if (it['type'] != 'track') return;
    widget.api.get('/library/metadata/$id/lyrics').then((l) {
      final lines = ((l is Map ? l['lines'] : null) as List?) ?? const [];
      if (!mounted || lines.isEmpty) return;
      setState(() {
        lyrics = [for (final x in lines) Map<String, dynamic>.from(x)];
        lyricsSynced = l['synced'] == true;
      });
    }).catchError((_) {}); // a song without words
  }

  Widget lyricsPanel() {
    if (lyrics.isEmpty || !lyricsOn) return const SizedBox.shrink();
    return Positioned(
      top: 40,
      bottom: 110,
      right: 16,
      width: 380,
      child: DecoratedBox(
          decoration: BoxDecoration(color: Colors.black54, borderRadius: BorderRadius.circular(8)),
          child: StreamBuilder<Duration>(
            stream: player.stream.position,
            builder: (_, snap) {
              final now = (snap.data ?? player.state.position).inMilliseconds;
              var cur = -1;
              if (lyricsSynced) {
                for (var i = 0; i < lyrics.length; i++) {
                  if (((lyrics[i]['t'] ?? 0) as num) <= now) cur = i;
                }
                if (cur >= 0 && lyricScroll.hasClients) {
                  final target = (cur * 34.0 - 100).clamp(0.0, lyricScroll.position.maxScrollExtent);
                  if ((lyricScroll.offset - target).abs() > 30) lyricScroll.animateTo(target, duration: const Duration(milliseconds: 300), curve: Curves.easeOut);
                }
              }
              return ListView.builder(
                controller: lyricScroll,
                padding: const EdgeInsets.all(12),
                itemExtent: lyricsSynced ? 34 : null,
                itemCount: lyrics.length,
                itemBuilder: (_, i) {
                  final x = lyrics[i];
                  final text = '${x['text'] ?? ''}';
                  return InkWell(
                    onTap: lyricsSynced && x['t'] != null ? () => player.seek(Duration(milliseconds: (x['t'] as num).toInt())) : null,
                    child: Text(text.isEmpty ? ' ' : text, style: TextStyle(fontSize: 16, color: !lyricsSynced || i == cur ? Colors.white : Colors.white54, fontWeight: i == cur ? FontWeight.w700 : FontWeight.normal)),
                  );
                },
              );
            },
          ),
        ),
    );
  }
  List<double> credits = []; // seconds
  List<double>? intro; // start, end in seconds
  Map<String, dynamic>? next;
  bool suggested = false; // the next film of a series: offered at the end, not started by itself
  late List<Map<String, dynamic>> queue = [...widget.queue];
  bool nextCard = false, stillWatching = false;
  int left = 0;
  bool restoredSub = false, ended = false;
  String method = '';

  // ---- Zusammen schauen: everyone in the room plays the same thing at the same place.
  Timer? tgTimer;
  int tgSeq = 0;
  DateTime tgQuiet = DateTime.now(); // changes before this are ours following the room, not told to the others
  dynamic tgMe;
  String tgNames = '';
  bool tgSwitching = false; // moving to another video of the room: do not leave it
  int tgFails = 0; // polls in a row that failed (a short Wi-Fi drop must not end the room)
  int tgLastPos = 0;
  DateTime tgLastAt = DateTime.now();

  Future<void> tgStart() async {
    final code = widget.together!;
    try {
      final me = await widget.api.get('/api/me');
      tgMe = me is Map ? me['id'] : null;
      final st = await widget.api.get('/api/together/$code');
      tgSeq = (st['seq'] as num).toInt();
      tgQuiet = DateTime.now().add(const Duration(seconds: 3));
      tgDraw(st);
      if (st['paused'] == true) await player.pause();
    } catch (e) {
      if (mounted) setState(() => error = '$e');
      return;
    }
    subs.add(player.stream.playing.listen((_) => tgSend()));
    tgLastPos = player.state.position.inMilliseconds;
    tgLastAt = DateTime.now();
    tgTimer = Timer.periodic(const Duration(seconds: 2), (_) => tgPoll());
  }

  void tgDraw(dynamic st) {
    final others = [for (final m in (st['members'] as List? ?? const [])) if (m['id'] != tgMe) '${m['name']}'];
    final text = others.isEmpty ? 'zusammen (wartet auf andere)' : 'zusammen mit ${others.join(', ')}';
    if (text != tgNames && mounted) {
      final first = tgNames.isEmpty;
      setState(() => tgNames = text);
      if (!first) ScaffoldMessenger.of(context).showSnackBar(SnackBar(content: Text('Zusammen schauen: ${others.isEmpty ? 'allein' : others.join(', ')}')));
    }
  }

  Future<void> tgSend({Map<String, dynamic> extra = const {}}) async {
    final code = widget.together;
    if (code == null || !mounted || DateTime.now().isBefore(tgQuiet)) return;
    try {
      final st = await widget.api.post('/api/together/$code', {'position': player.state.position.inMilliseconds, 'paused': !player.state.playing, ...extra});
      if (st is Map && st['seq'] is num) tgSeq = tgSeq > (st['seq'] as num).toInt() ? tgSeq : (st['seq'] as num).toInt();
    } catch (_) {}
  }

  Future<void> tgPoll() async {
    final code = widget.together;
    if (code == null || !mounted || tgSwitching) return;
    // A jump made here (the seek bar, the arrow keys) is told to the others.
    final pos = player.state.position.inMilliseconds;
    final expected = tgLastPos + (player.state.playing ? DateTime.now().difference(tgLastAt).inMilliseconds : 0);
    tgLastPos = pos;
    tgLastAt = DateTime.now();
    if ((pos - expected).abs() > 3000) tgSend();
    dynamic st;
    try {
      st = await widget.api.get('/api/together/$code');
      tgFails = 0;
    } catch (e) {
      if (++tgFails < 3) return;
      if (mounted) {
        ScaffoldMessenger.of(context).showSnackBar(SnackBar(content: Text('$e')));
        Navigator.maybePop(context);
      }
      return;
    }
    if (!mounted) return;
    tgDraw(st);
    final seq = (st['seq'] as num).toInt();
    if (seq <= tgSeq) return;
    tgSeq = seq;
    tgQuiet = DateTime.now().add(const Duration(seconds: 2));
    if ('${st['item']}' != id) {
      tgSwitching = true;
      ended = true;
      Navigator.pushReplacement(context, MaterialPageRoute(builder: (_) => PlayerScreen(api: widget.api, item: {'id': st['item'], 'ratingKey': '${st['item']}', 'viewOffset': st['position']}, together: code)));
      return;
    }
    final want = (st['position'] as num).toInt();
    if ((player.state.position.inMilliseconds - want).abs() > 2000) {
      await player.seek(Duration(milliseconds: want));
      tgLastPos = want;
      tgLastAt = DateTime.now();
    }
    if (st['paused'] == true && player.state.playing) await player.pause();
    if (st['paused'] != true && !player.state.playing) await player.play();
  }

  @override
  void initState() {
    super.initState();
    start();
  }

  Future<void> loadPrefs() async {
    final p = await SharedPreferences.getInstance();
    quality = qualityKey(p.getString('quality'));
    speed = p.getDouble('speed') ?? 1;
    subSize = subSizeKey(p.getInt('subSize'));
    autoNext = p.getBool('autoNext') ?? true;
    subLang = p.getString('subLang') ?? '';
    audioLang = p.getString('audioLang') ?? '';
  }

  Future<void> savePref(String k, Object v) => savePlayerPref(widget.api, k, v);

  Future<void> start() async {
    try {
      await loadPrefs();
      final full = await widget.api.item(id);
      it = {...full, if (widget.item['viewOffset'] != null) 'viewOffset': widget.item['viewOffset']};
      final media = (it['Media'] as List?)?.isNotEmpty ?? false ? Map<String, dynamic>.from((it['Media'] as List).first) : <String, dynamic>{};
      externalSubs = ((media['subtitles'] as List?) ?? const []).map((e) => Map<String, dynamic>.from(e)).where((s) => s['embedded'] != true && s['key'] != null).toList();
      if (externalSubs.isNotEmpty) subTicket = await widget.api.itemTicket(id);
      credits = ((it['credits'] as List?) ?? const []).map((e) => (e as num) / 1000).toList();
      final i = it['intro'];
      if (i is Map && (i['end'] ?? 0) > (i['start'] ?? 0)) intro = [(i['start'] as num) / 1000, (i['end'] as num) / 1000];
      widget.api.chapters(id).then((c) {
        if (mounted) setState(() => chapters = c);
      });
      findNext();
      loadLyrics();
      if (it['type'] == 'movie' && ((it['viewOffset'] ?? 0) as num) <= 0 && widget.together == null) await playPreroll();
      if (!mounted) return;
      await load(((it['viewOffset'] ?? 0) as num).toInt());
      subs.add(player.stream.error.listen((e) async {
        if (e.isEmpty || checkingError || error != null) return;
        // media_kit passes many mpv log lines on as errors, and some are not fatal (one failed HLS
        // segment, an external subtitle that does not load). While the film was playing, wait a moment:
        // if the picture keeps moving, mpv carried on and the error is ignored.
        if (player.state.playing) {
          checkingError = true;
          final at = player.state.position;
          await Future.delayed(const Duration(seconds: 3));
          checkingError = false;
          if (!mounted || error != null) return;
          if (player.state.playing && player.state.position - at >= const Duration(milliseconds: 1500)) {
            debugPrint('player: ignored an error, playback went on: $e');
            return;
          }
        }
        if (triedHls || quality != 'original') {
          await player.pause(); // the error screen replaces the picture; the sound must not go on behind it
          if (mounted) setState(() => error = e);
          return;
        }
        triedHls = true;
        await load(player.state.position.inMilliseconds, forceHls: true);
      }));
      subs.add(player.stream.completed.listen((c) {
        if (c) finished();
      }));
      subs.add(player.stream.tracks.listen((t) {
        if (!restoredAudio && audioLang.isNotEmpty) {
          final a = t.audio.where((x) => x.id != 'auto' && x.id != 'no' && _sameLang(x.language, audioLang)).firstOrNull;
          if (a != null) {
            restoredAudio = true;
            player.setAudioTrack(a);
          }
        }
        if (restoredSub || subLang.isEmpty) return;
        final cand = t.subtitle.where((s) => s.id != 'auto' && s.id != 'no' && _sameLang(s.language, subLang)).firstOrNull;
        if (cand != null) {
          restoredSub = true;
          pickedSub = cand;
          player.setSubtitleTrack(cand);
        }
      }));
      ticker = Timer.periodic(const Duration(seconds: 10), (_) => report(player.state.playing ? 'playing' : 'paused'));
      sleepTimer = Timer.periodic(const Duration(seconds: 1), (_) => sleepTick());
      if (widget.together != null) tgStart();
    } catch (e) {
      if (mounted) setState(() => error = '$e');
    }
  }

  Future<void> load(int atMs, {bool forceHls = false}) async {
    final d = await widget.api.decision(id, quality: quality, audio: audioIdx);
    method = '${d['method'] ?? ''}';
    final useHls = forceHls || quality != 'original' || audioIdx != null || (d['allowed'] == true && d['method'] == 'transcode');
    converted = useHls;
    final url = useHls ? '${d['hls']}' : '${d['direct'] ?? d['url']}';
    await player.open(Media(widget.api.abs(url), httpHeaders: widget.api.auth, start: Duration(milliseconds: atMs)));
    await player.setRate(speed);
    if (mounted) setState(() {});
  }

  Future<void> findNext() async {
    if (PlayerScreen.userQueue.isNotEmpty) {
      next = PlayerScreen.userQueue.first;
    } else if (queue.isNotEmpty) {
      next = queue.first;
    } else if (it['type'] == 'episode' && it['grandparentRatingKey'] != null) {
      try {
        final all = await widget.api.allLeaves('${it['grandparentRatingKey']}');
        final i = all.indexWhere((x) => '${x['ratingKey'] ?? x['id']}' == id);
        if (i >= 0 && i + 1 < all.length) {
          next = all[i + 1];
          queue = all.sublist(i + 2);
        }
      } catch (_) {}
    } else if (it['type'] == 'movie') {
      try {
        final hubs = await widget.api.related(id, count: 50);
        final hub = hubs.where((h) => h['hubIdentifier'] == 'movie.collection').firstOrNull;
        final year = ((it['year'] ?? 0) as num).toInt();
        final list = [
          for (final m in (hub?['Metadata'] as List? ?? const []))
            if (((m['viewCount'] ?? 0) as num) <= 0 && ((m['year'] ?? 9999) as num) >= year) Map<String, dynamic>.from(m)
        ];
        if (list.isNotEmpty) {
          next = list.first;
          suggested = true;
        }
      } catch (_) {}
    }
    if (mounted) setState(() {});
  }

  String nextName(Map<String, dynamic> n) => n['type'] == 'episode' ? 'S${n['parentIndex'] ?? 0}E${n['index'] ?? 0} · ${n['title']}' : '${n['title']}';

  Future<void> report(String state) async {
    final r = await widget.api.timeline(id, player.state.position.inMilliseconds, state);
    if (r != null && r['stopped'] == true && mounted) stoppedByOwner('${r['message'] ?? ''}');
  }

  void stoppedByOwner(String message) {
    ticker?.cancel();
    player.pause();
    showDialog(
      context: context,
      builder: (c) => AlertDialog(
        title: const Text('Wiedergabe beendet'),
        content: Column(mainAxisSize: MainAxisSize.min, crossAxisAlignment: CrossAxisAlignment.start, children: [
          const Text('Der Besitzer des Servers hat deine Wiedergabe beendet.'),
          if (message.isNotEmpty) Padding(padding: const EdgeInsets.only(top: 12), child: Text(message)),
        ]),
        actions: [TextButton(onPressed: () => Navigator.pop(c), child: const Text('OK'))],
      ),
    ).then((_) {
      if (mounted) Navigator.pop(context);
    });
  }

  /// Played to the end: watched, then the next one (by itself, or asking), or close.
  void finished() {
    if (ended) return;
    ended = true;
    widget.api.scrobble(id);
    widget.api.timeline(id, 0, 'stopped');
    if (next == null || sleepAtEnd()) {
      if (mounted) Navigator.pop(context);
      return;
    }
    if (!autoNext || suggested) {
      setState(() => nextCard = true);
    } else if (widget.autoRun >= _stillWatchingAfter) {
      askStillWatching();
    } else {
      startCountdown(10, () => playNext(auto: true));
    }
  }

  // ---------------------------------------------------------------- sleep timer

  static const sleepMinutes = [15, 30, 45, 60, 90, 120];
  static const sleepFadeSecs = 20;

  /// null: off; 0: at the end of this title; else minutes from now.
  void setSleep(int? minutes) {
    sleepRestore();
    PlayerScreen.sleepEnd = minutes == 0;
    PlayerScreen.sleepAt = minutes == null || minutes == 0 ? null : DateTime.now().add(Duration(minutes: minutes));
  }

  String sleepLabel() {
    if (PlayerScreen.sleepEnd) return 'Ende des Titels';
    final at = PlayerScreen.sleepAt;
    if (at == null) return 'Aus';
    return 'noch ${(at.difference(DateTime.now()).inSeconds / 60).ceil()} Min.';
  }

  void sleepRestore() {
    final v = sleepVol;
    sleepVol = null;
    if (v != null) player.setVolume(v);
  }

  /// At the end of the title: stop instead of going on, when the sleep timer says so.
  bool sleepAtEnd() {
    if (!PlayerScreen.sleepEnd) return false;
    PlayerScreen.sleepEnd = false;
    tell('Sleep-Timer: nach diesem Titel beendet. Gute Nacht!');
    return true;
  }

  void sleepTick() {
    if (!mounted || error != null) return;
    if (PlayerScreen.sleepEnd) {
      // The credits count as the end, like "Abspann überspringen".
      final pos = player.state.position.inMilliseconds / 1000, d = player.state.duration.inMilliseconds / 1000;
      if (!ended && player.state.playing && credits.isNotEmpty && pos >= credits.first && pos < d - 3) {
        ended = true;
        widget.api.scrobble(id);
        widget.api.timeline(id, 0, 'stopped');
        sleepAtEnd();
        Navigator.pop(context);
      }
      return;
    }
    final at = PlayerScreen.sleepAt;
    if (at == null) return;
    final left = at.difference(DateTime.now()).inMilliseconds / 1000;
    if (left <= 0) {
      PlayerScreen.sleepAt = null;
      player.pause().then((_) => sleepRestore());
      report('paused');
      tell('Sleep-Timer: angehalten. Gute Nacht!');
    } else if (left <= sleepFadeSecs && player.state.playing) {
      sleepVol ??= player.state.volume;
      player.setVolume(sleepVol! * left / sleepFadeSecs);
    }
  }

  void startCountdown(int secs, VoidCallback done) {
    countdown?.cancel();
    setState(() {
      nextCard = true;
      left = secs;
    });
    countdown = Timer.periodic(const Duration(seconds: 1), (t) {
      if (--left <= 0) {
        t.cancel();
        done();
      } else if (mounted) {
        setState(() {});
      }
    });
  }

  void askStillWatching() {
    setState(() => stillWatching = true);
    startCountdown(30, () {
      if (mounted) Navigator.pop(context);
    });
  }

  void playNext({bool auto = false}) {
    final n = next;
    if (n == null) return;
    countdown?.cancel();
    if (!ended) {
      // Skipped ahead: counts as watched, like letting the credits run.
      widget.api.scrobble(id);
      widget.api.timeline(id, 0, 'stopped');
    }
    ended = true;
    goingOn = true;
    if (widget.together != null) {
      tgSwitching = true;
      tgQuiet = DateTime.now();
      widget.api.post('/api/together/${widget.together}', {'item': '${n['ratingKey'] ?? n['id']}', 'position': 0, 'paused': false}).catchError((_) {});
    }
    PlayerScreen.userQueue.removeWhere((x) => '${x['ratingKey'] ?? x['id']}' == '${n['ratingKey'] ?? n['id']}');
    Navigator.pushReplacement(
      context,
      MaterialPageRoute(builder: (_) => PlayerScreen(api: widget.api, item: n, queue: queue.isNotEmpty && queue.first == n ? queue.sublist(1) : queue, autoRun: auto ? widget.autoRun + 1 : 0, together: widget.together)),
    );
  }

  @override
  void dispose() {
    tgTimer?.cancel();
    if (widget.together != null && !tgSwitching) widget.api.delete('/api/together/${widget.together}').catchError((_) {});
    ticker?.cancel();
    countdown?.cancel();
    sleepTimer?.cancel();
    if (!goingOn) {
      PlayerScreen.sleepAt = null;
      PlayerScreen.sleepEnd = false;
    }
    ticksTimer?.cancel();
    lyricScroll.dispose();
    ticksOn.dispose();
    hoverPos.dispose();
    for (final s in subs) {
      s.cancel();
    }
    if (!ended) widget.api.timeline(id, player.state.position.inMilliseconds, 'stopped');
    player.dispose();
    super.dispose();
  }

  // ---------------------------------------------------------------- menus

  Future<void> openMenu([String page = 'main']) async {
    await showDialog(
      context: context,
      builder: (_) => _SettingsMenu(state: this, page: page),
    );
    if (mounted) setState(() {});
  }

  Future<void> setQuality(String q) async {
    quality = q;
    savePref('quality', q);
    triedHls = false;
    await load(player.state.position.inMilliseconds);
  }

  Future<void> setSpeed(double v) async {
    speed = v;
    savePref('speed', v);
    await player.setRate(v);
  }

  void setSubSize(int v) {
    subSize = v;
    savePref('subSize', v);
    setState(() {});
  }

  Future<void> pickSubtitle(SubtitleTrack t, {String lang = ''}) async {
    pickedSub = t;
    await player.setSubtitleTrack(t);
    subLang = lang;
    restoredSub = true;
    savePref('subLang', lang);
  }

  /// Fetches a subtitle file again after its timing changed (the address gets a new tail so nothing is reused).
  Future<void> reloadSub(Map x) async {
    final t = SubtitleTrack.uri(widget.api.subtitleUrl('${x['key']}', subTicket, extra: 'r=${DateTime.now().millisecondsSinceEpoch}'), title: '${x['title'] ?? ''}', language: '${x['language'] ?? ''}');
    pickedSub = t;
    await player.setSubtitleTrack(t);
  }

  void tell(String m) {
    if (mounted) ScaffoldMessenger.of(context).showSnackBar(SnackBar(content: Text(m), duration: const Duration(seconds: 4)));
  }

  /// Owner: listen to the sound and move the subtitle lines onto the speech.
  Future<void> syncSub(Map x) async {
    tell('Höre auf den Ton … das dauert einen Moment');
    try {
      final r = await widget.api.post('${x['key']}/sync');
      if (r['applied'] != true) return tell('${r['message'] ?? 'Nicht sicher genug, nichts verschoben'}');
      await reloadSub(x);
      tell('Untertitel um ${((r['offsetMs'] ?? 0) / 1000).toStringAsFixed(1).replaceAll('.', ',')} s verschoben');
    } catch (e) {
      tell('$e');
    }
  }

  /// Owner: move a subtitle file by [ms], or back to how it was (null).
  Future<void> nudgeSub(Map x, int? ms) async {
    try {
      final r = await widget.api.put('${x['key']}/timing', ms == null ? {'reset': true} : {'delta_ms': ms});
      await reloadSub(x);
      tell(ms == null ? 'Untertitel-Zeit zurückgesetzt' : 'Untertitel um ${(((r['offsetMs'] ?? 0) as num) / 1000).toStringAsFixed(1).replaceAll('.', ',')} s verschoben');
    } catch (e) {
      tell('$e');
    }
  }

  String trackName(dynamic t) {
    final parts = <String>[
      if ((t.language ?? '').toString().isNotEmpty) '${t.language}',
      if ((t.title ?? '').toString().isNotEmpty) '${t.title}',
    ];
    return parts.isEmpty ? 'Spur ${t.id}' : parts.join(' · ');
  }

  // ---------------------------------------------------------------- build

  Widget overlay() {
    return StreamBuilder<Duration>(
      stream: player.stream.position,
      builder: (context, snap) {
        final pos = (snap.data ?? player.state.position).inMilliseconds / 1000;
        final inIntro = intro != null && pos >= intro![0] && pos < intro![1];
        final inCredits = next != null && credits.isNotEmpty && pos >= credits.first && !nextCard;
        return Stack(children: [
          if (inIntro) Positioned(right: 24, bottom: 110, child: FilledButton.tonal(onPressed: () => player.seek(Duration(milliseconds: (intro![1] * 1000).round())), child: const Text('Intro überspringen'))),
          if (inCredits && !inIntro)
            Positioned(right: 24, bottom: 110, child: FilledButton.tonal(onPressed: () => playNext(), child: Text('Nächste Folge: ${nextName(next!)}'))),
          if (nextCard && next != null) Positioned(right: 24, bottom: 110, child: _nextCard()),
        ]);
      },
    );
  }

  Widget _nextCard() {
    final n = next!;
    return Card(
      color: Colors.black87,
      child: Padding(
        padding: const EdgeInsets.all(12),
        child: SizedBox(
          width: 340,
          child: Row(crossAxisAlignment: CrossAxisAlignment.start, children: [
            SizedBox(width: 90, child: AspectRatio(aspectRatio: 16 / 9, child: Image.network(widget.api.img(n['thumb']), headers: widget.api.auth, fit: BoxFit.cover, cacheWidth: 240, errorBuilder: (_, __, ___) => Container(color: Colors.white10)))),
            const SizedBox(width: 12),
            Expanded(
              child: Column(crossAxisAlignment: CrossAxisAlignment.start, children: [
                if (stillWatching)
                  Text('Schaust du noch? · stoppt in $left s', style: const TextStyle(fontWeight: FontWeight.bold))
                else
                  Text('Als Nächstes${countdown?.isActive == true ? ' · in $left s' : ''}', style: const TextStyle(color: Colors.white60, fontSize: 12)),
                Text(n['type'] == 'episode' ? '${n['grandparentTitle'] ?? ''}' : '${n['title']}', maxLines: 1, overflow: TextOverflow.ellipsis, style: const TextStyle(fontWeight: FontWeight.w600)),
                if (n['type'] == 'episode') Text(nextName(n), maxLines: 1, overflow: TextOverflow.ellipsis, style: const TextStyle(fontSize: 12)),
                const SizedBox(height: 8),
                Row(children: [
                  FilledButton(onPressed: () => playNext(), child: Text(stillWatching ? 'Ja, weiter' : 'Jetzt abspielen')),
                  const SizedBox(width: 8),
                  TextButton(
                    onPressed: () {
                      countdown?.cancel();
                      if (stillWatching || ended) {
                        Navigator.pop(context);
                      } else {
                        setState(() => nextCard = false);
                      }
                    },
                    child: Text(stillWatching ? 'Stopp' : 'Abbrechen'),
                  ),
                ]),
              ]),
            ),
          ]),
        ),
      ),
    );
  }

  @override
  Widget build(BuildContext context) {
    final title = it['type'] == 'episode' ? '${it['grandparentTitle'] ?? ''} · S${it['parentIndex'] ?? 0}E${it['index'] ?? 0} · ${it['title']}' : '${it['title'] ?? widget.item['title'] ?? ''}';
    final top = <Widget>[
      MaterialDesktopCustomButton(icon: const Icon(Icons.arrow_back), onPressed: () => Navigator.maybePop(context)),
      Expanded(child: Padding(padding: const EdgeInsets.symmetric(horizontal: 8), child: Text(tgNames.isEmpty ? title : '$title · $tgNames', maxLines: 1, overflow: TextOverflow.ellipsis, style: const TextStyle(fontSize: 16)))),
      if (method.isNotEmpty) Text(const {'direct': 'Direkt', 'remux': 'Umgepackt', 'transcode': 'Umgewandelt'}[method] ?? method, style: const TextStyle(fontSize: 12, color: Colors.white60)),
    ];
    final theme = MaterialDesktopVideoControlsThemeData(
      topButtonBar: top,
      bottomButtonBar: [
        const MaterialDesktopPlayOrPauseButton(),
        if (next != null) MaterialDesktopCustomButton(icon: const Icon(Icons.skip_next), onPressed: () => playNext()),
        const MaterialDesktopVolumeButton(),
        const MaterialDesktopPositionIndicator(),
        if (chapters.isNotEmpty)
          StreamBuilder<Duration>(
            stream: player.stream.position,
            builder: (_, snap) {
              final ms = (snap.data ?? player.state.position).inMilliseconds;
              var name = '';
              for (final c in chapters) {
                if (((c['start'] ?? 0) as num) <= ms) name = '${c['title']}'.replaceFirstMapped(RegExp(r'^Chapter (\d+)$'), (m) => 'Kapitel ${m[1]}');
              }
              return name.isEmpty ? const SizedBox() : Padding(padding: const EdgeInsets.only(left: 12), child: Text('· $name', style: const TextStyle(fontSize: 12, color: Colors.white70)));
            },
          ),
        const Spacer(),
        if (chapters.isNotEmpty) MaterialDesktopCustomButton(icon: const Icon(Icons.list), onPressed: () => openMenu('chapters')),
        if (lyrics.isNotEmpty) MaterialDesktopCustomButton(icon: Icon(lyricsOn ? Icons.lyrics : Icons.lyrics_outlined), onPressed: () => setState(() => lyricsOn = !lyricsOn)),
        MaterialDesktopCustomButton(icon: const Icon(Icons.closed_caption_outlined), onPressed: () => openMenu('subs')),
        MaterialDesktopCustomButton(icon: const Icon(Icons.settings), onPressed: () => openMenu()),
        const MaterialDesktopFullscreenButton(),
      ],
      keyboardShortcuts: {
        const SingleActivator(LogicalKeyboardKey.space): () => player.playOrPause(),
        const SingleActivator(LogicalKeyboardKey.arrowLeft): () => player.seek(back10(player.state.position)),
        const SingleActivator(LogicalKeyboardKey.arrowRight): () => player.seek(player.state.position + const Duration(seconds: 30)),
        const SingleActivator(LogicalKeyboardKey.arrowUp): () => player.setVolume((player.state.volume + 5).clamp(0, 100)),
        const SingleActivator(LogicalKeyboardKey.arrowDown): () => player.setVolume((player.state.volume - 5).clamp(0, 100)),
        const SingleActivator(LogicalKeyboardKey.keyM): () => player.setVolume(player.state.volume > 0 ? 0 : 100),
        const SingleActivator(LogicalKeyboardKey.keyN): () => playNext(),
        const SingleActivator(LogicalKeyboardKey.keyC): () => openMenu('subs'),
        const SingleActivator(LogicalKeyboardKey.keyF): () => toggleFullscreen(context),
        const SingleActivator(LogicalKeyboardKey.escape): () => isFullscreen(context) ? exitFullscreen(context) : Navigator.maybePop(context),
      },
    );
    // Phones and tablets: the desktop controls only appear for a mouse, so touch gets media_kit's
    // touch controls (tap shows them, double tap on the sides skips) with the same buttons.
    final touch = defaultTargetPlatform == TargetPlatform.iOS || defaultTargetPlatform == TargetPlatform.android;
    final mobile = MaterialVideoControlsThemeData(
      seekOnDoubleTap: true,
      seekOnDoubleTapForwardDuration: const Duration(seconds: 30),
      topButtonBar: [MaterialCustomButton(icon: const Icon(Icons.arrow_back), onPressed: () => Navigator.maybePop(context)), ...top.skip(1)],
      primaryButtonBar: [
        const Spacer(flex: 2),
        MaterialCustomButton(icon: const Icon(Icons.replay_10), iconSize: 36, onPressed: () => player.seek(back10(player.state.position))),
        const Spacer(),
        const MaterialPlayOrPauseButton(iconSize: 48),
        const Spacer(),
        MaterialCustomButton(icon: const Icon(Icons.forward_30), iconSize: 36, onPressed: () => player.seek(player.state.position + const Duration(seconds: 30))),
        const Spacer(flex: 2),
      ],
      bottomButtonBar: [
        const MaterialPositionIndicator(),
        const Spacer(),
        if (next != null) MaterialCustomButton(icon: const Icon(Icons.skip_next), onPressed: () => playNext()),
        if (chapters.isNotEmpty) MaterialCustomButton(icon: const Icon(Icons.list), onPressed: () => openMenu('chapters')),
        if (lyrics.isNotEmpty) MaterialCustomButton(icon: Icon(lyricsOn ? Icons.lyrics : Icons.lyrics_outlined), onPressed: () => setState(() => lyricsOn = !lyricsOn)),
        MaterialCustomButton(icon: const Icon(Icons.closed_caption_outlined), onPressed: () => openMenu('subs')),
        MaterialCustomButton(icon: const Icon(Icons.settings), onPressed: () => openMenu()),
        const MaterialFullscreenButton(),
      ],
    );
    return Scaffold(
      backgroundColor: Colors.black,
      body: error != null
          ? Center(
              child: Column(mainAxisSize: MainAxisSize.min, children: [
                Text('Wiedergabe fehlgeschlagen:\n$error', textAlign: TextAlign.center),
                const SizedBox(height: 12),
                FilledButton(onPressed: () => Navigator.pop(context), child: const Text('Zurück')),
              ]),
            )
          : MaterialVideoControlsTheme(
              normal: mobile,
              fullscreen: mobile,
              child: MaterialDesktopVideoControlsTheme(
                normal: theme,
                fullscreen: theme,
                child: Video(
                  controller: controller,
                  subtitleViewConfiguration: SubtitleViewConfiguration(
                    style: TextStyle(fontSize: 32 * subSize / 100, color: Colors.white, backgroundColor: const Color(0x99000000), height: 1.3),
                  ),
                  controls: (state) => Listener(
                    onPointerHover: (e) { mouseMoved(); hoverPos.value = e.localPosition; },
                    onPointerDown: (_) => mouseMoved(),
                    // Nothing under the notch, the status bar or the home indicator.
                    child: SafeArea(
                      child: Stack(children: [
                        // The chapter marks and the still over the seek bar are placed for the desktop bar.
                        if (touch) MaterialVideoControls(state) else ...[MaterialDesktopVideoControls(state), chapterTicks(), seekTip()],
                        lyricsPanel(),
                        prerollButtons(),
                        overlay(),
                      ]),
                    ),
                  ),
                ),
              ),
            ),
    );
  }
}

/// The player's settings menu: quality, speed, audio, subtitles (+ size), chapters, "next episode by itself".
class _SettingsMenu extends StatefulWidget {
  final _PlayerScreenState state;
  final String page;
  const _SettingsMenu({required this.state, required this.page});
  @override
  State<_SettingsMenu> createState() => _SettingsMenuState();
}

class _SettingsMenuState extends State<_SettingsMenu> {
  late String page = widget.page;
  _PlayerScreenState get s => widget.state;
  Player get p => s.player;

  Widget choice(String label, bool on, VoidCallback pick) => ListTile(
        dense: true,
        leading: SizedBox(width: 24, child: on ? const Icon(Icons.check, size: 18) : null),
        title: Text(label),
        onTap: pick,
      );

  Widget entry(String label, String value, String go) => ListTile(dense: true, title: Text(label), trailing: Text('$value  ›', style: const TextStyle(color: Colors.white60)), onTap: () => setState(() => page = go));

  Widget back(String title) => ListTile(dense: true, leading: const Icon(Icons.chevron_left), title: Text(title, style: const TextStyle(fontWeight: FontWeight.bold)), onTap: () => setState(() => page = 'main'));

  Widget head(String t) => Padding(padding: const EdgeInsets.fromLTRB(16, 10, 16, 2), child: Text(t, style: const TextStyle(fontWeight: FontWeight.bold, color: Colors.white70)));

  @override
  Widget build(BuildContext context) {
    final tracks = p.state.tracks;
    final audios = tracks.audio.where((t) => t.id != 'auto' && t.id != 'no').toList();
    // A converted stream carries one audio track: the others are asked from the server.
    final fileAudios = [
      for (final x in ((s.it['Media'] as List?)?.isNotEmpty ?? false ? (s.it['Media'][0]['streams'] as List? ?? const []) : const []))
        if (x['kind'] == 'audio') Map<String, dynamic>.from(x)
    ];
    final serverPick = s.converted && fileAudios.length > 1;
    final embedded = tracks.subtitle.where((t) => t.id != 'auto' && t.id != 'no').toList();
    final tr = p.state.track.audio;
    // While the player is on "auto" it plays the preferred language, else the first track: show that one.
    final curAudio = audios.any((t) => t.id == tr.id)
        ? tr
        : (audios.where((t) => s.audioLang.isNotEmpty && _sameLang(t.language, s.audioLang)).firstOrNull ?? (audios.isNotEmpty ? audios.first : tr));
    final curSub = s.pickedSub ?? p.state.track.subtitle;
    final children = <Widget>[];
    switch (page) {
      case 'subs':
        children.addAll([
          head('Untertitel'),
          choice('Aus', curSub.id == 'no', () {
            s.pickSubtitle(SubtitleTrack.no());
            Navigator.pop(context);
          }),
          for (final t in embedded)
            choice(s.trackName(t), curSub.id == t.id, () {
              s.pickSubtitle(t, lang: t.language ?? '');
              Navigator.pop(context);
            }),
          for (final x in s.externalSubs)
            choice('${x['language'] ?? x['file'] ?? 'Datei'}${x['title'] != null ? ' · ${x['title']}' : ''}', curSub.uri && curSub.id.startsWith('${s.widget.api.abs('${x['key']}')}?'), () {
              s.pickSubtitle(SubtitleTrack.uri(s.widget.api.subtitleUrl('${x['key']}', s.subTicket), title: '${x['title'] ?? ''}', language: '${x['language'] ?? ''}'), lang: '${x['language'] ?? ''}');
              Navigator.pop(context);
            }),
          if (Poster.admin)
            for (final x in s.externalSubs)
              if (curSub.uri && curSub.id.startsWith('${s.widget.api.abs('${x['key']}')}?')) ...[
                head('Zeit der Untertitel'),
                ListTile(dense: true, title: const Text('Mit dem Ton abgleichen'), onTap: () { Navigator.pop(context); s.syncSub(x); }),
                ListTile(dense: true, title: const Text('0,5 s früher'), onTap: () { Navigator.pop(context); s.nudgeSub(x, -500); }),
                ListTile(dense: true, title: const Text('0,5 s später'), onTap: () { Navigator.pop(context); s.nudgeSub(x, 500); }),
                ListTile(dense: true, title: const Text('Zeit zurücksetzen'), onTap: () { Navigator.pop(context); s.nudgeSub(x, null); }),
              ],
          head('Untertitelgröße'),
          for (final e in playerSubSizes.entries)
            choice(e.value, s.subSize == e.key, () {
              s.setSubSize(e.key);
              setState(() {});
            }),
        ]);
      case 'quality':
        children.addAll([
          back('Qualität'),
          for (final e in playerQualities.entries)
            choice(e.value, s.quality == e.key, () {
              Navigator.pop(context);
              s.setQuality(e.key);
            }),
        ]);
      case 'speed':
        children.addAll([
          back('Wiedergabegeschwindigkeit'),
          for (final v in _speeds)
            choice(v == 1 ? 'Normal' : '${v.toString().replaceAll('.', ',')}×', s.speed == v, () {
              s.setSpeed(v);
              setState(() {});
            }),
        ]);
      case 'audio':
        children.addAll([
          back('Tonspur'),
          if (serverPick)
            for (final a in fileAudios)
              choice(
                [a['language'] ?? 'Ton', '${a['codec'] ?? ''}'.toUpperCase(), if ((a['channels'] ?? 0) != 0) '${a['channels']}ch', a['title'] ?? ''].where((e) => '$e'.isNotEmpty).join(' · '),
                (s.audioIdx ?? fileAudios.first['index']) == a['index'],
                () {
                  Navigator.pop(context);
                  s.audioIdx = (a['index'] as num).toInt();
                  s.load(s.player.state.position.inMilliseconds);
                },
              )
          else
          for (final t in audios)
            choice(s.trackName(t), curAudio.id == t.id, () {
              p.setAudioTrack(t);
              Navigator.pop(context);
            }),
        ]);
      case 'sleep':
        void pick(int? m) {
          s.setSleep(m);
          Navigator.pop(context);
          if (m != null) s.tell('Sleep-Timer: ${s.sleepLabel()}');
        }
        children.addAll([
          back('Sleep-Timer'),
          choice('Aus', PlayerScreen.sleepAt == null && !PlayerScreen.sleepEnd, () => pick(null)),
          choice(s.it['type'] == 'episode' ? 'Ende dieser Folge' : 'Ende dieses Films', PlayerScreen.sleepEnd, () => pick(0)),
          for (final m in _PlayerScreenState.sleepMinutes) choice('$m Minuten', false, () => pick(m)),
        ]);
      case 'chapters':
        children.addAll([
          back('Kapitel'),
          for (final c in s.chapters)
            ListTile(
              dense: true,
              selected: s.chapters.lastWhere((x) => ((x['start'] ?? 0) as num) <= p.state.position.inMilliseconds, orElse: () => s.chapters.first) == c,
              title: Text('${c['title']}'.replaceFirstMapped(RegExp(r'^Chapter (\d+)$'), (m) => 'Kapitel ${m[1]}')),
              trailing: Text(fmtClock((c['start'] ?? 0) as int), style: const TextStyle(color: Colors.white60)),
              onTap: () {
                p.seek(Duration(milliseconds: (c['start'] ?? 0) as int));
                Navigator.pop(context);
              },
            ),
        ]);
      default:
        children.addAll([
          entry('Qualität', playerQualities[s.quality] ?? s.quality, 'quality'),
          entry('Wiedergabegeschwindigkeit', s.speed == 1 ? 'Normal' : '${s.speed.toString().replaceAll('.', ',')}×', 'speed'),
          if (serverPick || audios.length > 1) entry('Tonspur', serverPick ? 'wählen' : audios.where((t) => t.id == curAudio.id).map(s.trackName).firstOrNull ?? 'Standard', 'audio'),
          entry('Untertitel', curSub.id == 'no' ? 'Aus' : s.trackName(curSub), 'subs'),
          if (s.chapters.isNotEmpty) entry('Kapitel', '${s.chapters.length}', 'chapters'),
          entry('Sleep-Timer', s.sleepLabel(), 'sleep'),
          if (s.it['type'] == 'episode' || s.queue.isNotEmpty)
            choice('Nächste Folge automatisch abspielen', s.autoNext, () {
              s.autoNext = !s.autoNext;
              s.savePref('autoNext', s.autoNext);
              setState(() {});
            }),
        ]);
    }
    final size = MediaQuery.of(context).size;
    return Dialog(
      alignment: Alignment.bottomRight,
      insetPadding: const EdgeInsets.fromLTRB(16, 16, 16, 80),
      child: ConstrainedBox(
        constraints: BoxConstraints(maxWidth: 260, maxHeight: (size.height - 100).clamp(160.0, 300.0)),
        child: ListTileTheme(
          data: const ListTileThemeData(dense: true, minVerticalPadding: 0, visualDensity: VisualDensity(vertical: -3), titleTextStyle: TextStyle(fontSize: 13, color: Colors.white), horizontalTitleGap: 6),
          child: ListView(shrinkWrap: true, padding: const EdgeInsets.symmetric(vertical: 4), children: children),
        ),
      ),
    );
  }
}
