// Lichtspiel web app: languages.
//
// The English text in app.js and pages.js is the key: t('Sign in') gives the
// text of the chosen language pack, or the English text itself when the pack
// has none. {name} in a text is filled from the second argument:
//   t('Scanning {name}', { name: 'Movies' })
// A language pack (web/lang/<code>.js) calls I18N.add(code, name, { strings, patterns }):
//   strings  – { 'English text': 'translation' }
//   patterns – [[/regex on the English text/, 'replacement' or function], …] for
//              text the server builds itself ("More by Christopher Nolan").
// English needs no pack. The choice is kept per browser (localStorage "lang");
// without one the browser's own language is used when a pack for it exists.
'use strict';
const I18N = {
  packs: { en: { name: 'English', strings: {}, patterns: [] } },
  lang: 'en',
  add(code, name, pack) {
    this.packs[code] = { name, strings: pack.strings || {}, patterns: pack.patterns || [] };
    this.lang = this.pick();
    document.documentElement.lang = this.lang;
  },
  pick() {
    let saved = null;
    try { saved = localStorage.getItem('lang'); } catch (e) { /* storage blocked */ }
    if (saved && this.packs[saved]) return saved;
    for (const l of navigator.languages || [navigator.language || 'en']) {
      const c = String(l).toLowerCase().split('-')[0];
      if (this.packs[c]) return c;
    }
    return 'en';
  },
  set(code) {
    try { localStorage.setItem('lang', code); } catch (e) { /* storage blocked */ }
    location.reload();
  },
  // Elements in index.html carry data-t (their text), data-t-title / data-t-placeholder.
  applyStatic(root = document) {
    root.querySelectorAll('[data-t]').forEach(el => { el.textContent = t(el.dataset.t); });
    root.querySelectorAll('[data-t-title]').forEach(el => { el.title = t(el.dataset.tTitle); });
    root.querySelectorAll('[data-t-placeholder]').forEach(el => { el.placeholder = t(el.dataset.tPlaceholder); });
  },
};

function t(s, vars) {
  if (s === null || s === undefined) return s;
  s = String(s);
  const p = I18N.packs[I18N.lang];
  let out = s;
  if (p && p !== I18N.packs.en) {
    if (Object.prototype.hasOwnProperty.call(p.strings, s)) out = p.strings[s];
    else for (const [re, rep] of p.patterns) { if (re.test(s)) { out = s.replace(re, rep); break; } }
  }
  return vars ? out.replace(/\{(\w+)\}/g, (m, k) => (k in vars ? vars[k] : m)) : out;
}

// The language select shown in the top bar and on the sign-in pages.
function langSelect() {
  const sel = document.createElement('select');
  sel.title = t('Language');
  sel.className = 'lang';
  for (const [code, p] of Object.entries(I18N.packs)) {
    const o = document.createElement('option');
    o.value = code; o.textContent = p.name; o.selected = code === I18N.lang;
    sel.append(o);
  }
  sel.addEventListener('change', () => I18N.set(sel.value));
  return sel;
}
