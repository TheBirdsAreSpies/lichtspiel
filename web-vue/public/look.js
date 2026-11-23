// Light or dark and the accent before the first paint (no flash): the choice kept in this browser
// (src/look.ts keeps it there; a file of its own, as the page allows no inline scripts).
(function () {
  var d = document.documentElement, c = 'system', a = 'rose';
  try { c = localStorage.getItem('theme') || c; a = localStorage.getItem('accent') || a; } catch (e) { /* storage blocked */ }
  var light = c === 'light' || (c === 'system' && window.matchMedia && matchMedia('(prefers-color-scheme: light)').matches);
  d.setAttribute('data-theme', c === 'oled' ? 'oled' : light ? 'light' : 'dark');
  d.setAttribute('data-accent', a);
})();
