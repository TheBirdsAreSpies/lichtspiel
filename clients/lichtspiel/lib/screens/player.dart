import 'dart:async';
import 'package:flutter/material.dart';
import 'package:flutter/services.dart';
import 'package:media_kit/media_kit.dart';
import 'package:media_kit_video/media_kit_video.dart';
import '../api.dart';

/// Plays through libmpv, which handles mkv/HEVC/HDR/ASS directly, so the server's direct URL is used.
/// Falls back to the server's HLS stream if the direct file fails. Reports progress every 10 s.
class PlayerScreen extends StatefulWidget {
  final Api api;
  final Map<String, dynamic> item;
  const PlayerScreen({super.key, required this.api, required this.item});
  @override
  State<PlayerScreen> createState() => _PlayerScreenState();
}

class _PlayerScreenState extends State<PlayerScreen> {
  final player = Player();
  late final VideoController controller = VideoController(player);
  Timer? ticker;
  String? error;
  bool triedHls = false;
  String get id => '${widget.item['ratingKey']}';

  @override
  void initState() {
    super.initState();
    start();
  }

  Future<void> start() async {
    try {
      final d = await widget.api.decision(id);
      final resume = (widget.item['viewOffset'] ?? d['resume'] ?? 0) as int;
      await open(widget.api.abs('${d['direct'] ?? d['url']}'), resume);
      player.stream.error.listen((e) async {
        if (triedHls) return setState(() => error = e);
        triedHls = true;
        final pos = player.state.position.inMilliseconds;
        await open(widget.api.abs('${d['hls']}'), pos);
      });
      player.stream.completed.listen((c) {
        if (c) {
          widget.api.scrobble(id);
          widget.api.timeline(id, 0, 'stopped');
          if (mounted) Navigator.pop(context);
        }
      });
      ticker = Timer.periodic(const Duration(seconds: 10), (_) => report(player.state.playing ? 'playing' : 'paused'));
    } catch (e) {
      setState(() => error = '$e');
    }
  }

  Future<void> open(String url, int resumeMs) async {
    await player.open(Media(url, start: Duration(milliseconds: resumeMs)));
  }

  void report(String state) => widget.api.timeline(id, player.state.position.inMilliseconds, state);

  @override
  void dispose() {
    ticker?.cancel();
    report('stopped');
    player.dispose();
    super.dispose();
  }

  @override
  Widget build(BuildContext context) {
    return Scaffold(
      backgroundColor: Colors.black,
      body: error != null
          ? Center(child: Column(mainAxisSize: MainAxisSize.min, children: [
              Text('Wiedergabe fehlgeschlagen:\n$error', textAlign: TextAlign.center),
              const SizedBox(height: 12),
              FilledButton(onPressed: () => Navigator.pop(context), child: const Text('Zurück')),
            ]))
          : CallbackShortcuts(
              bindings: {
                const SingleActivator(LogicalKeyboardKey.space): player.playOrPause,
                const SingleActivator(LogicalKeyboardKey.arrowLeft): () => player.seek(player.state.position - const Duration(seconds: 10)),
                const SingleActivator(LogicalKeyboardKey.arrowRight): () => player.seek(player.state.position + const Duration(seconds: 30)),
                const SingleActivator(LogicalKeyboardKey.escape): () => Navigator.pop(context),
              },
              child: Focus(autofocus: true, child: Video(controller: controller)),
            ),
    );
  }
}
