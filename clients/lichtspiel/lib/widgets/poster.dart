import 'package:flutter/material.dart';
import '../api.dart';
import 'activity.dart';

/// A poster card with title, subtitle, watched mark and progress bar. Focusable (D-pad / remote ready).
class Poster extends StatelessWidget {
  final Api api;
  final Map<String, dynamic> item;
  final VoidCallback onTap;
  final double width;
  const Poster({super.key, required this.api, required this.item, required this.onTap, this.width = 140});

  @override
  Widget build(BuildContext context) {
    final isEp = item['type'] == 'episode';
    final title = isEp ? (item['grandparentTitle'] ?? item['title']) : item['title'];
    final sub = isEp ? 'S${item['parentIndex'] ?? '?'}E${item['index'] ?? '?'} · ${item['title']}' : ([item['year']?.toString(), item['editionTitle']?.toString()].whereType<String>().where((e) => e.isNotEmpty).join(' · '));
    final dur = (item['duration'] ?? 0) as int;
    final off = (item['viewOffset'] ?? 0) as int;
    final thumb = isEp && item['grandparentThumb'] != null ? item['grandparentThumb'] : item['thumb'];
    final watched = ((item['viewCount'] ?? 0) as int) > 0;
    return SizedBox(
      width: width,
      child: InkWell(
        onTap: onTap,
        borderRadius: BorderRadius.circular(6),
        child: Column(crossAxisAlignment: CrossAxisAlignment.start, children: [
          AspectRatio(
            aspectRatio: 2 / 3,
            child: ClipRRect(
              borderRadius: BorderRadius.circular(6),
              child: Stack(fit: StackFit.expand, children: [
                Container(color: Colors.white10),
                Image.network(api.img(thumb), fit: BoxFit.cover, errorBuilder: (_, __, ___) => Center(child: Text('$title', textAlign: TextAlign.center))),
                BusyOverlay(item: item),
                if (watched) const Positioned(top: 4, right: 4, child: Icon(Icons.check_circle, size: 18, color: Color(0xFFE5A00D))),
                if (off > 0 && dur > 0)
                  Positioned(left: 0, right: 0, bottom: 0, child: LinearProgressIndicator(value: (off / dur).clamp(0, 1).toDouble(), minHeight: 3)),
              ]),
            ),
          ),
          const SizedBox(height: 6),
          Text('$title', maxLines: 1, overflow: TextOverflow.ellipsis, style: const TextStyle(fontWeight: FontWeight.w500)),
          Text(sub, maxLines: 1, overflow: TextOverflow.ellipsis, style: const TextStyle(fontSize: 12, color: Colors.white54)),
        ]),
      ),
    );
  }
}
