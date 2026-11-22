import 'dart:async';
import 'package:flutter/material.dart';
import '../api.dart';

/// Polls what the server is updating right now (admins), so the poster of that title can show a spinner.
class Activity {
  static final busy = ValueNotifier<Set<int>>({});
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
        final w = st is Map ? (st['working_ids'] ?? (st['working_on'] != null ? [st['working_on']] : [])) : [];
        final now = <int>{for (final x in (w as List)) if (x is num) x.toInt()};
        if (now.length != busy.value.length || !now.containsAll(busy.value)) busy.value = now;
        if (busy.value.isNotEmpty || (st is Map && ((st['queued'] ?? 0) as num) > 0)) wait = const Duration(milliseconds: 2500);
      } catch (_) {
        if (busy.value.isNotEmpty) busy.value = {};
      }
      if (_running) _timer = Timer(wait, tick);
    }

    tick();
  }

  static void stop() {
    _running = false;
    _timer?.cancel();
    busy.value = {};
  }
}

/// Dims the picture and shows a spinner while the server updates the details of [item].
class BusyOverlay extends StatelessWidget {
  final Map<String, dynamic> item;
  const BusyOverlay({super.key, required this.item});

  @override
  Widget build(BuildContext context) {
    final id = int.tryParse('${item['id'] ?? item['ratingKey']}') ?? -1;
    return ValueListenableBuilder<Set<int>>(
      valueListenable: Activity.busy,
      builder: (_, b, _) => !b.contains(id)
          ? const SizedBox.shrink()
          : Positioned.fill(
              child: Container(color: Colors.black45, child: const Center(child: SizedBox(width: 28, height: 28, child: CircularProgressIndicator(strokeWidth: 3)))),
            ),
    );
  }
}
