import 'package:flutter/material.dart';
import '../api.dart';
import '../theme.dart';

/// What went wrong, in words for the user: the server's and the network's errors are German already,
/// anything else (a bug, an answer of the wrong shape) is not shown raw.
String errorText(Object e) {
  if (e is ApiError) return e.message;
  debugPrint('Unexpected error: $e');
  return 'Etwas ist schiefgelaufen';
}

/// A failed load: what went wrong and a button to try again. With a plain text as [error] it is a quiet
/// note instead (an empty list, nothing found).
class ErrorView extends StatelessWidget {
  final Object error;
  final VoidCallback? onRetry;
  final IconData? icon;
  const ErrorView({super.key, required this.error, this.onRetry, this.icon});

  @override
  Widget build(BuildContext context) {
    final offline = error is ApiError && (error as ApiError).status == 0;
    return Center(
      child: Padding(
        padding: const EdgeInsets.all(32),
        child: Column(mainAxisSize: MainAxisSize.min, children: [
          Icon(icon ?? (offline ? Icons.cloud_off_outlined : error is String ? Icons.inbox_outlined : Icons.error_outline), size: 48, color: context.ghost),
          const SizedBox(height: 12),
          Text(error is String ? error as String : errorText(error), textAlign: TextAlign.center, style: TextStyle(color: context.dim)),
          if (onRetry != null) ...[
            const SizedBox(height: 16),
            FilledButton.tonalIcon(onPressed: onRetry, icon: const Icon(Icons.refresh), label: const Text('Erneut versuchen')),
          ],
        ]),
      ),
    );
  }
}

/// A short message with "Rückgängig".
void undoSnack(BuildContext context, String text, VoidCallback onUndo) {
  final m = ScaffoldMessenger.of(context);
  m.hideCurrentSnackBar();
  m.showSnackBar(SnackBar(content: Text(text), action: SnackBarAction(label: 'Rückgängig', onPressed: onUndo)));
}

/// Grey boxes that slowly pulse while the real content loads.
class Skeleton extends StatefulWidget {
  final Widget child;
  const Skeleton({super.key, required this.child});
  @override
  State<Skeleton> createState() => _SkeletonState();
}

class _SkeletonState extends State<Skeleton> with SingleTickerProviderStateMixin {
  late final c = AnimationController(vsync: this, duration: const Duration(milliseconds: 900))..repeat(reverse: true);

  @override
  void dispose() {
    c.dispose();
    super.dispose();
  }

  @override
  Widget build(BuildContext context) => FadeTransition(opacity: Tween(begin: .45, end: 1.0).animate(c), child: widget.child);
}

/// One grey box of a skeleton.
class Bone extends StatelessWidget {
  final double? width, height;
  final double radius;
  const Bone({super.key, this.width, this.height, this.radius = 6});
  @override
  Widget build(BuildContext context) => Container(width: width, height: height, decoration: BoxDecoration(color: context.hair2, borderRadius: BorderRadius.circular(radius)));
}

/// A poster's place: the picture and two lines.
class PosterBone extends StatelessWidget {
  final double width;
  const PosterBone({super.key, this.width = 140});
  @override
  Widget build(BuildContext context) => SizedBox(
        width: width,
        child: Column(crossAxisAlignment: CrossAxisAlignment.start, children: [
          const AspectRatio(aspectRatio: 2 / 3, child: Bone()),
          const SizedBox(height: 8),
          Bone(width: width * .8, height: 12),
          const SizedBox(height: 6),
          Bone(width: width * .45, height: 10),
        ]),
      );
}

/// The home screen while it loads: a few rows of posters.
class RowsSkeleton extends StatelessWidget {
  const RowsSkeleton({super.key});
  @override
  Widget build(BuildContext context) => Skeleton(
        child: ListView(
          physics: const NeverScrollableScrollPhysics(),
          padding: const EdgeInsets.symmetric(vertical: 12),
          children: [
            for (var r = 0; r < 3; r++) ...[
              const Padding(padding: EdgeInsets.fromLTRB(16, 20, 16, 10), child: Align(alignment: Alignment.centerLeft, child: Bone(width: 160, height: 16))),
              SizedBox(
                height: 280,
                child: ListView.separated(
                  physics: const NeverScrollableScrollPhysics(),
                  scrollDirection: Axis.horizontal,
                  padding: const EdgeInsets.symmetric(horizontal: 16),
                  itemCount: 8,
                  separatorBuilder: (_, _) => const SizedBox(width: 12),
                  itemBuilder: (_, _) => const PosterBone(),
                ),
              ),
            ],
          ],
        ),
      );
}

/// A library or list while it loads: a grid of posters.
class GridSkeleton extends StatelessWidget {
  const GridSkeleton({super.key});
  @override
  Widget build(BuildContext context) => Skeleton(
        child: GridView.builder(
          physics: const NeverScrollableScrollPhysics(),
          padding: const EdgeInsets.all(16),
          gridDelegate: const SliverGridDelegateWithMaxCrossAxisExtent(maxCrossAxisExtent: 170, childAspectRatio: 0.48, crossAxisSpacing: 12, mainAxisSpacing: 12),
          itemCount: 24,
          itemBuilder: (_, _) => LayoutBuilder(builder: (_, b) => PosterBone(width: b.maxWidth)),
        ),
      );
}
