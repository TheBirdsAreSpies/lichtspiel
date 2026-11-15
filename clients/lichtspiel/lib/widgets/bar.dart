import 'package:flutter/material.dart';

/// One button of a screen's top bar.
class BarAction {
  final IconData icon;
  final String label;
  final VoidCallback onPressed;
  const BarAction(this.icon, this.label, this.onPressed);
}

/// Phones (and narrow windows) don't have room for a row of buttons in the top bar.
bool isNarrow(BuildContext context) => MediaQuery.sizeOf(context).width < 700;

/// The top bar's buttons: side by side when there is room ([labels] shows their names too),
/// otherwise the first [keep] as icons and the rest in a "⋮" menu.
List<Widget> barActions(BuildContext context, List<BarAction> actions, {bool labels = false, int keep = 0}) {
  if (!isNarrow(context)) {
    return [
      for (final a in actions)
        labels ? TextButton.icon(icon: Icon(a.icon), label: Text(a.label), onPressed: a.onPressed) : IconButton(tooltip: a.label, icon: Icon(a.icon), onPressed: a.onPressed),
    ];
  }
  final shown = actions.take(keep).toList();
  final rest = actions.skip(keep).toList();
  return [
    for (final a in shown) IconButton(tooltip: a.label, icon: Icon(a.icon), onPressed: a.onPressed),
    if (rest.isNotEmpty)
      PopupMenuButton<int>(
        tooltip: 'Mehr',
        onSelected: (i) => rest[i].onPressed(),
        itemBuilder: (_) => [
          for (var i = 0; i < rest.length; i++) PopupMenuItem(value: i, child: ListTile(contentPadding: EdgeInsets.zero, leading: Icon(rest[i].icon), title: Text(rest[i].label))),
        ],
      ),
  ];
}
