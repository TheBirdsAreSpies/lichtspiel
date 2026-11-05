import 'dart:async';
import 'package:flutter/material.dart';
import '../api.dart';

/// Polls what the server is updating right now (admins), so the poster of that title can show a spinner.
class Activity {
  static final busy = ValueNotifier<int>(0);
  static Timer? _timer;
  static bool _running = false;

  static void start(Api api) {
    if (_running) return;
    _running = true;
    Future<void> tick() async {
      if (!_running) return;
      var wait = const Duration(seconds: 8);
      try {
        final st = await api.get('/api/metadata/status');
        final w = st is Map ? st['working_on'] : null;
        busy.value = w is num ? w.toInt() : 0;
        if (busy.value != 0 || (st is Map && ((st['queued'] ?? 0) as num) > 0)) wait = const Duration(milliseconds: 2500);
      } catch (_) {
        busy.value = 0;
      }
      if (_running) _timer = Timer(wait, tick);
    }

    tick();
  }

  static void stop() {
    _running = false;
    _timer?.cancel();
    busy.value = 0;
  }
}

/// Dims the picture and shows a spinner while the server updates the details of [item].
class BusyOverlay extends StatelessWidget {
  final Map<String, dynamic> item;
  const BusyOverlay({super.key, required this.item});

  @override
  Widget build(BuildContext context) {
    final id = int.tryParse('${item['id'] ?? item['ratingKey']}') ?? -1;
    return ValueListenableBuilder<int>(
      valueListenable: Activity.busy,
      builder: (_, b, __) => b != id
          ? const SizedBox.shrink()
          : Positioned.fill(
              child: Container(color: Colors.black45, child: const Center(child: SizedBox(width: 28, height: 28, child: CircularProgressIndicator(strokeWidth: 3)))),
            ),
    );
  }
}
