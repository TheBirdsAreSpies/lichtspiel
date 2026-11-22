import 'package:flutter/foundation.dart';
import 'package:flutter/services.dart';
import 'package:flutter/material.dart';
import '../api.dart';
import '../screens/edit.dart';
import '../screens/player.dart';
import 'activity.dart';
import 'together.dart';
import 'item_extras.dart';
import 'states.dart';
import '../screens/playlists.dart' show addToPlaylistDialog;
import '../screens/downloads.dart';
import '../theme.dart';

/// A poster card with title, subtitle, watched mark and progress bar. Focusable (D-pad / remote ready):
/// the focused poster grows a little and gets a frame in the accent colour.
/// With the mouse over it (or focused), a play button, (admins) an edit pencil and a "…" menu appear, as in the web app.
/// On a phone or tablet the picture stays clear: a tap opens the title, a long press opens the menu.
class Poster extends StatefulWidget {
  /// Set once the signed-in person is known to be the owner.
  static bool admin = false;

  final Api api;
  final Map<String, dynamic> item;
  final VoidCallback onTap;
  final double width;
  /// Multiple selection (libraries): [onPick] toggles this poster, [picked] says whether it is chosen,
  /// [selecting] that something is chosen already, so a tap picks instead of opening.
  final VoidCallback? onPick;
  final bool picked;
  final bool selecting;
  /// Set in the "Continue Watching" row: the menu offers to take the title off it, then calls [onRemoved].
  final VoidCallback? onRemoved;
  /// Replaces the line under the title (the value a library is sorted by, like rating or length).
  final String? caption;
  const Poster({super.key, required this.api, required this.item, required this.onTap, this.width = 140, this.onPick, this.picked = false, this.selecting = false, this.onRemoved, this.caption});

  @override
  State<Poster> createState() => _PosterState();
}

class _PosterState extends State<Poster> {
  bool hover = false;
  bool focused = false;

  Map<String, dynamic> get item => widget.item;
  String get id => '${item['ratingKey'] ?? item['id']}';
  bool get playable => const {'movie', 'episode', 'track', 'clip'}.contains(item['type']);
  bool get touch => defaultTargetPlatform == TargetPlatform.android || defaultTargetPlatform == TargetPlatform.iOS;

  bool get watched {
    final leaves = (item['leafCount'] ?? 0) as num;
    if (leaves > 0) return ((item['viewedLeafCount'] ?? 0) as num) >= leaves;
    return ((item['viewCount'] ?? 0) as num) > 0;
  }

  void say(String m) {
    if (mounted) ScaffoldMessenger.of(context).showSnackBar(SnackBar(content: Text(m)));
  }

  /// Fetch what the server has now, so the card shows the change.
  Future<void> reload() async {
    try {
      final fresh = await widget.api.item(id);
      item.addAll(fresh);
      if (mounted) setState(() {});
    } catch (_) {}
  }

  Future<void> play() async {
    await Navigator.push(context, MaterialPageRoute(builder: (_) => PlayerScreen(api: widget.api, item: item)));
    reload();
  }

  Future<void> menu(Offset at) async {
    final matchable = item['type'] == 'movie' || item['type'] == 'show';
    final matched = item['extra']?['metadata_source'] != null;
    final size = MediaQuery.of(context).size;
    final v = await showMenu<String>(
      context: context,
      position: RelativeRect.fromLTRB(at.dx, at.dy, size.width - at.dx, size.height - at.dy),
      items: [
        if (touch && playable && item['missing'] != true) const PopupMenuItem(value: 'play', child: Text('Abspielen')),
        if (widget.onPick != null && !widget.picked) const PopupMenuItem(value: 'pick', child: Text('Auswählen')),
        if (playable && !(item['missing'] == true)) const PopupMenuItem(value: 'queue', child: Text('Zur Warteschlange')),
        if (widget.onRemoved != null) const PopupMenuItem(value: 'unresume', child: Text('Aus „Weiterschauen“ entfernen')),
        if (playable && item['missing'] != true && item['type'] != 'track') const PopupMenuItem(value: 'together', child: Text('Zusammen schauen…')),
        if (DownloadRights.allowed && (playable || const {'show', 'season'}.contains(item['type'])) && item['missing'] != true) const PopupMenuItem(value: 'download', child: Text('Herunterladen…')),
        if (const {'movie', 'show', 'episode', 'track'}.contains(item['type'])) const PopupMenuItem(value: 'playlist', child: Text('Zur Playlist…')),
        if (const {'movie', 'show', 'season', 'episode'}.contains(item['type'])) const PopupMenuItem(value: 'share', child: Text('Link kopieren')),
        if (item['type'] != 'clip' && item['type'] != 'collection') const PopupMenuItem(value: 'history', child: Text('Verlauf')),
        if (playable) const PopupMenuItem(value: 'report', child: Text('Problem melden…')),
        PopupMenuItem(value: 'watched', child: Text(watched ? 'Als ungesehen markieren' : 'Als gesehen markieren')),
        if (Poster.admin && matchable) const PopupMenuItem(value: 'refresh', child: Text('Metadaten aktualisieren')),
        if (Poster.admin) const PopupMenuItem(value: 'edit', child: Text('Details bearbeiten')),
        if (Poster.admin && matchable) PopupMenuItem(value: 'match', child: Text(matched ? 'Zuordnung korrigieren…' : 'Zuordnen…')),
      ],
    );
    if (v == null || !mounted) return;
    try {
      switch (v) {
        case 'play':
          await play();
          return;
        case 'pick':
          widget.onPick?.call();
          return;
        case 'queue':
          PlayerScreen.userQueue.add(item);
          say('Zur Warteschlange hinzugefügt – sie läuft nach dem aktuellen Video');
          return;
        case 'playlist':
          await addToPlaylistDialog(context, widget.api, id, item: item);
          return;
        case 'share':
          await Clipboard.setData(ClipboardData(text: '${widget.api.base}/#/item/$id'));
          say('Link kopiert – wer ein Konto hier hat und die Mediathek sehen darf, kann ihn öffnen');
          return;
        case 'history':
          await historyDialog(context, widget.api, item);
          return;
        case 'report':
          await reportDialog(context, widget.api, item);
          return;
        case 'download':
          await downloadDialog(context, widget.api, [item]);
          return;
        case 'together':
          await watchTogether(context, widget.api, item);
          return;
        case 'unresume':
          await widget.api.post('/api/continue/remove', {'ids': [id]});
          say('Aus „Weiterschauen“ entfernt');
          widget.onRemoved?.call();
          return;
        case 'watched':
          final was = watched;
          await widget.api.setWatched(id, !was);
          if (mounted) {
            undoSnack(context, was ? 'Als ungesehen markiert' : 'Als gesehen markiert', () async {
              try {
                await widget.api.setWatched(id, was);
              } catch (e) {
                say(errorText(e));
              }
              await reload();
            });
          }
        case 'refresh':
          say('Lade Details…');
          final r = await widget.api.post('/library/metadata/$id/refresh');
          if (r is Map && r['message'] != null) say('${r['message']}');
        case 'edit':
          await edit();
          return;
        case 'match':
          final item = await widget.api.item(id);
          if (!mounted) return;
          await matchDialog(context, widget.api, item);
      }
    } catch (e) {
      say(errorText(e));
    }
    await reload();
  }

  /// Long press: the menu on a phone (at the poster), choosing while several are being chosen.
  void longPress() {
    if (widget.onPick != null && (widget.selecting || !touch)) return widget.onPick!();
    final b = context.findRenderObject() as RenderBox?;
    if (b == null) return;
    menu(b.localToGlobal(b.size.center(Offset.zero)));
  }

  Future<void> edit() async {
    final item = await widget.api.item(id);
    if (!mounted) return;
    await editDetailsDialog(context, widget.api, item);
    await reload();
  }

  Widget round(IconData icon, String tip, VoidCallback onTap, {double size = 18, double pad = 6}) => Tooltip(
        message: tip,
        child: Material(
          color: Colors.black54,
          shape: const CircleBorder(),
          child: InkWell(customBorder: const CircleBorder(), onTap: onTap, child: Padding(padding: EdgeInsets.all(pad), child: Icon(icon, size: size, color: Colors.white))),
        ),
      );

  @override
  Widget build(BuildContext context) {
    final isEp = item['type'] == 'episode';
    final title = isEp ? (item['grandparentTitle'] ?? item['title']) : item['title'];
    final sub = isEp ? 'S${item['parentIndex'] ?? '?'}E${item['index'] ?? '?'} · ${item['title']}' : ([item['year']?.toString(), item['editionTitle']?.toString()].whereType<String>().where((e) => e.isNotEmpty).join(' · '));
    final dur = ((item['duration'] ?? 0) as num).toInt();
    final off = ((item['viewOffset'] ?? 0) as num).toInt();
    final thumb = isEp && item['grandparentThumb'] != null ? item['grandparentThumb'] : item['thumb'];
    final missing = item['missing'] == true;
    final leaves = ((item['leafCount'] ?? 0) as num).toInt();
    final unseen = leaves > 0 ? leaves - ((item['viewedLeafCount'] ?? 0) as num).toInt() : 0;
    final show = hover || focused;
    final cache = (widget.width * MediaQuery.devicePixelRatioOf(context)).round();
    return SizedBox(
      width: widget.width,
      child: MouseRegion(
        onEnter: (_) => setState(() => hover = true),
        onExit: (_) => setState(() => hover = false),
        child: GestureDetector(
          onSecondaryTapUp: (d) => menu(d.globalPosition),
          child: InkWell(
            onFocusChange: (f) => setState(() => focused = f),
            onTap: () {
              final pick = widget.onPick;
              if (pick != null && (widget.selecting || HardwareKeyboard.instance.isControlPressed || HardwareKeyboard.instance.isShiftPressed)) {
                pick();
              } else {
                widget.onTap();
              }
            },
            onLongPress: longPress,
            borderRadius: BorderRadius.circular(6),
            child: Column(crossAxisAlignment: CrossAxisAlignment.start, children: [
              AnimatedScale(
                scale: focused ? 1.05 : 1,
                duration: const Duration(milliseconds: 120),
                child: AspectRatio(
                aspectRatio: 2 / 3,
                child: ClipRRect(
                  borderRadius: BorderRadius.circular(6),
                  child: Stack(fit: StackFit.expand, children: [
                    Container(color: context.hair),
                    Image.network(widget.api.img(thumb), headers: widget.api.auth, fit: BoxFit.cover, cacheWidth: cache, errorBuilder: (_, _, _) => Center(child: Text('$title', textAlign: TextAlign.center))),
                    if (missing) Container(color: Colors.black54),
                    BusyOverlay(item: item),
                    if (missing) const Center(child: Tooltip(message: 'Datei fehlt – im Papierkorb der Mediathek', child: Icon(Icons.delete, size: 56, color: Colors.redAccent, shadows: [Shadow(blurRadius: 6, color: Colors.black87)]))),
                    if (unseen > 0)
                      Positioned(top: 6, right: 6, child: Container(padding: const EdgeInsets.symmetric(horizontal: 7), decoration: BoxDecoration(color: context.accent, borderRadius: BorderRadius.circular(10)), child: Text('$unseen', style: TextStyle(color: context.onAccent, fontSize: 12, fontWeight: FontWeight.w700))))
                    else if (watched)
                      Positioned(
                        top: 6,
                        right: 6,
                        child: Container(
                          width: 24,
                          height: 24,
                          decoration: BoxDecoration(shape: BoxShape.circle, color: const Color(0xB80E1013), border: Border.all(color: context.accent, width: 1.5)),
                          child: Icon(Icons.check, size: 15, color: context.accent),
                        ),
                      ),
                    if (focused && !widget.picked) Positioned.fill(child: DecoratedBox(decoration: BoxDecoration(border: Border.all(color: context.accent, width: 3), borderRadius: BorderRadius.circular(6)))),
                    if (widget.picked) Positioned.fill(child: DecoratedBox(decoration: BoxDecoration(border: Border.all(color: context.accent, width: 3), borderRadius: BorderRadius.circular(6), color: context.accent.withValues(alpha: .2)))),
                    if (widget.onPick != null && (widget.picked || widget.selecting || show))
                      Positioned(
                        top: 6,
                        left: 6,
                        child: GestureDetector(
                          onTap: widget.onPick,
                          child: Container(
                            width: 24,
                            height: 24,
                            decoration: BoxDecoration(shape: BoxShape.circle, color: widget.picked ? context.accent : Colors.black54, border: Border.all(color: widget.picked ? context.accent : Colors.white70, width: 1.5)),
                            child: widget.picked ? Icon(Icons.check, size: 16, color: context.onAccent) : null,
                          ),
                        ),
                      ),
                    if (off > 0 && dur > 0)
                      Positioned(left: 0, right: 0, bottom: 0, child: LinearProgressIndicator(value: (off / dur).clamp(0, 1).toDouble(), minHeight: 3)),
                    if (show && playable && !missing) Center(child: round(Icons.play_arrow, 'Abspielen', play, size: 30, pad: 10)),
                    if (show && Poster.admin && item['type'] != 'clip') Positioned(left: 6, bottom: 10, child: round(Icons.edit_outlined, 'Details bearbeiten', edit)),
                    if (show && item['type'] != 'clip')
                      Positioned(
                        right: 6,
                        bottom: 10,
                        child: Builder(builder: (c) => round(Icons.more_vert, 'Mehr', () {
                              final b = c.findRenderObject() as RenderBox;
                              menu(b.localToGlobal(Offset.zero));
                            })),
                      ),
                  ]),
                ),
              ),
              ),
              const SizedBox(height: 6),
              Text('$title', maxLines: 1, overflow: TextOverflow.ellipsis, style: const TextStyle(fontWeight: FontWeight.w500)),
              Text(widget.caption != null && widget.caption!.isNotEmpty ? widget.caption! : sub, maxLines: 1, overflow: TextOverflow.ellipsis, style: TextStyle(fontSize: 12, color: context.faint)),
            ]),
          ),
        ),
      ),
    );
  }
}
