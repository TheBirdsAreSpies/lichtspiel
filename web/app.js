// Lichtspiel web app, part 1: helpers, sign-in, browsing, details. The library page is in library.js,
// the player in player.js. Talks only to the server's JSON API (see #/api). Part 2 is pages.js.
// Every text shown goes through t() (i18n.js); the language packs are in web/lang/.
'use strict';
// Where the sign-in is kept: for good (the default), or only until the browser is closed
// (Settings -> Web -> sign in automatically: off).
const keepSignedIn = () => { try { return localStorage.getItem('autoLogin') !== '0'; } catch (e) { return true; } };
function saveToken(tok) {
  try {
    if (keepSignedIn()) { localStorage.setItem('token', tok); sessionStorage.removeItem('token'); }
    else { sessionStorage.setItem('token', tok); localStorage.removeItem('token'); }
  } catch (e) { /* private window: the sign-in lasts for this page */ }
}
// A link with ?token=… (made by the status window and the self-test) signs this browser in once (kept as
// "sign in automatically" says), then the token is taken out of the address bar.
(() => {
  const u = new URL(location.href); const tok = u.searchParams.get('token');
  if (tok) { saveToken(tok); u.searchParams.delete('token'); history.replaceState(null, '', u.pathname + u.search + u.hash); }
})();
function loadToken() { try { return sessionStorage.getItem('token') || localStorage.getItem('token') || ''; } catch (e) { return ''; } }
const S = { token: loadToken(), me: null, sections: [], filters: null, status: null };
const $ = (s, r = document) => r.querySelector(s);
const main = () => $('#main');
// How a person is shown to others: the full name, or the user name when none is set.
const shownName = u => (u && (u.display_name || u.username)) || '?';

function h(tag, attrs, ...kids) {
  const el = document.createElement(tag);
  for (const [k, v] of Object.entries(attrs || {})) {
    if (v === null || v === undefined || v === false) continue;
    if (k.startsWith('on')) el.addEventListener(k.slice(2), v);
    else if (k === 'class') el.className = v;
    else if (k === 'style') el.style.cssText = v;
    else if (k === 'value') el.value = v;
    else if (k === 'checked' || k === 'selected' || k === 'disabled') el[k] = !!v;
    else el.setAttribute(k, v === true ? '' : v);
  }
  for (const c of kids.flat(9)) if (c !== null && c !== undefined && c !== false) el.append(c instanceof Node ? c : String(c));
  return el;
}
// A form line: the label on the left, the box or menu next to it, an optional help text under the box.
// top = the label sits at the top (for text areas and lists of ticks).
let formIds = 0;
function formRow(label, control, help, top) {
  if (control instanceof HTMLElement && /^(INPUT|SELECT|TEXTAREA)$/.test(control.tagName) && !control.id) control.id = 'fld' + (++formIds);
  // A single tick box: the box in front of its text, the help below it (everywhere the same).
  if (control instanceof HTMLElement && control.tagName === 'INPUT' && control.type === 'checkbox') return checkRow(control, label, help);
  return h('div', { class: 'field' + (top ? ' top' : '') }, h('label', control && control.id ? { for: control.id } : {}, label),
    h('div', { class: 'ctl' }, control), help ? h('div', { class: 'help dim' }, help) : null);
}
// A tick box with its text after it and an optional help line below, lined up under the text.
function checkRow(box, label, help) {
  return h('div', { class: 'field check' }, h('label', { class: 'chk' }, box, h('span', {}, label)), help ? h('div', { class: 'help dim' }, help) : null);
}
// The management pages (libraries, users, …) are tabs of the settings and show their tab bar.
const MANAGE_PAGES = ['libraries', 'users', 'plugins', 'reports', 'api'];
function set(...kids) {
  const list = kids.flat(9).filter(Boolean);
  clearTimeout(S.skel);
  if (typeof NAV !== 'undefined' && MANAGE_PAGES.includes(NAV.cur) && S.me && S.me.admin) list.unshift(settingsNav(NAV.cur));
  // The tab bars scroll sideways on a phone: keep where they were, and keep the chosen tab in view.
  // Drawn again quietly after going back (see render()): into a box off the page, compared afterwards.
  if (S.offscreen) return S.offscreen.replaceChildren(...list);
  const kept = [...main().querySelectorAll('.tabs')].map(e => e.scrollLeft);
  main().replaceChildren(...list); window.scrollTo(0, 0);
  motion();
  main().querySelectorAll('.tabs').forEach((bar, i) => {
    if (kept[i]) bar.scrollLeft = kept[i];
    const on = bar.querySelector('a.on'); if (!on || bar.scrollWidth <= bar.clientWidth) return;
    const b = bar.getBoundingClientRect(), o = on.getBoundingClientRect();
    if (o.left < b.left || o.right > b.right) bar.scrollLeft += o.left - b.left - (b.width - o.width) / 2;
  });
}
// ---------- motion
// A new page fades in; a poster clicked in a list grows into the poster of the title's page while the rest
// of that page fades in beside it. Only on a change of page (MOTION.enter, set on hashchange), never when a
// page is drawn again in place after a change on it, and not at all when the system asks for less motion.
const MOTION = { enter: 0, from: null };
window.addEventListener('hashchange', () => { MOTION.enter = Date.now(); });
const calm = () => !window.matchMedia || matchMedia('(prefers-reduced-motion: reduce)').matches;
// From transparent to whatever the element has (the page's backdrop is only faintly visible, for one).
const fadeIn = (el, delay = 0, shift = '') => el && el.animate([{ opacity: 0, transform: shift || 'none', offset: 0 }], { duration: 320, delay, easing: 'cubic-bezier(.2,.8,.2,1)', fill: 'backwards' });
function motion() {
  if (!MOTION.enter) return;
  // Grey placeholders first: the page that replaces them is still the new page and gets the motion.
  const skel = !!main().querySelector('.skel');
  const fresh = Date.now() - MOTION.enter < 4000;
  if (!skel) MOTION.enter = 0;
  if (!fresh || calm()) { if (!skel) MOTION.from = null; return; }
  const from = MOTION.from;
  const to = !skel && from && Date.now() - from.at < 2500 && main().querySelector(`.detail > .poster[data-id="${from.id}"]`);
  if (!skel) MOTION.from = null;
  const b = to && to.getBoundingClientRect(), a = from && from.rect;
  if (!to || !a.width || !b.width) {
    main().animate([{ opacity: 0 }, { opacity: 1 }], { duration: 200, easing: 'ease-out' });
    if (!skel && !MOTION.back) { MOTION.stagger = Date.now(); stagger(); }
    return;
  }
  to.style.zIndex = 5;
  to.animate([{ transformOrigin: 'top left', transform: `translate(${a.left - b.left}px,${a.top - b.top}px) scale(${a.width / b.width},${a.height / b.height})`, borderRadius: '8px' },
    { transformOrigin: 'top left', transform: 'none' }], { duration: 440, easing: 'cubic-bezier(.2,.8,.2,1)' }).onfinish = () => { to.style.zIndex = ''; };
  for (const el of main().children) if (!el.contains(to) && !el.matches('img.hero')) fadeIn(el, 120);
  fadeIn(to.parentNode.querySelector('.info'), 60, 'translateX(18px)');
}
// The posters in view come in one after the other (row by row, left to right); the rest are simply there.
// A page that adds its posters a moment after it is drawn (a library) calls it again then: only once,
// and only right after the page was opened (not going back to it).
function stagger() {
  if (!MOTION.stagger || Date.now() - MOTION.stagger > 1500) return;
  const W = innerWidth, H = innerHeight;
  const seen = [...main().querySelectorAll('.card:not(.ph)')].filter(c => { const r = c.getBoundingClientRect(); return r.width && r.top < H && r.bottom > 0 && r.left < W && r.right > 0; });
  if (!seen.length) return;
  MOTION.stagger = 0;
  // All in view, however many a big screen shows; with many the steps get shorter, so the last is in within a second.
  const step = Math.min(28, 900 / seen.length);
  seen.forEach((c, i) => fadeIn(c, 60 + i * step, 'translateY(14px)'));
}
// ---------- arrow keys from poster to poster, like on a TV
// With a poster (or another of these) focused (Tab gets there), an arrow key moves to the nearest one in
// that direction: across a row, into the next row or grid, up to the banner's or the title's buttons.
// Enter opens it. Anywhere else the arrows scroll the page as always.
const NAV_TARGETS = '#main a.card, #main .card a.t, #main a.person, #main .spot-act > *, #main .stabs button,#main .item-actions > button';
const NAV_DIRS = { ArrowLeft: [-1, 0], ArrowRight: [1, 0], ArrowUp: [0, -1], ArrowDown: [0, 1] };
document.addEventListener('keydown', e => {
  const dir = NAV_DIRS[e.key];
  if (!dir || e.ctrlKey || e.metaKey || e.altKey || e.shiftKey || e.defaultPrevented) return;
  if ((typeof P !== 'undefined' && P.it) || document.querySelector('.modal, .menu')) return;
  const cur = document.activeElement;
  if (!cur || !cur.matches || !cur.matches(NAV_TARGETS)) return;
  const a = cur.getBoundingClientRect(), ax = a.left + a.width / 2, ay = a.top + a.height / 2;
  const found = [];
  for (const el of document.querySelectorAll(NAV_TARGETS)) {
    if (el === cur || el.disabled || !el.offsetParent) continue;
    const r = el.getBoundingClientRect();
    if (!r.width) continue;
    const x = r.left + r.width / 2, y = r.top + r.height / 2;
    if ((dir[0] ? (x - ax) * dir[0] : (y - ay) * dir[1]) <= 2) continue;
    if (dir[0]) {
      // Sideways only along the same line (overlapping heights): at a row's end the key does nothing, as on a TV.
      if (!(r.bottom > a.top + 4 && r.top < a.bottom - 4)) continue;
      found.push({ el, line: 0, near: Math.abs(x - ax) });
    } else {
      // Up or down: the next line first (how far its edge is), then the nearest in it; one in view beats
      // one scrolled away in its row.
      const gap = dir[1] > 0 ? r.top - a.bottom : a.top - r.bottom;
      found.push({ el, line: Math.max(0, gap), near: Math.max(0, r.left - a.right, a.left - r.right) + Math.abs(x - ax) / 1000 + (r.right < 0 || r.left > innerWidth ? 5000 : 0) });
    }
  }
  const first = Math.min(...found.map(f => f.line));
  let best = null, score = Infinity;
  for (const f of found) if (f.line <= first + 24 && f.near < score) { score = f.near; best = f.el; }
  if (!best) return; // nothing that way: the page scrolls
  e.preventDefault();
  best.focus({ preventScroll: true });
  best.scrollIntoView({ block: 'nearest', inline: 'nearest', behavior: calm() ? 'auto' : 'smooth' });
});
// A short message at the bottom; with action ({ label, run }) a button in it ("Undo"), and it stays a little longer.
function toast(msg, ms = 2600, action) {
  const el = $('#toast'), hide = () => (el.style.display = 'none');
  el.replaceChildren(msg);
  if (action) {
    ms = Math.max(ms, 6000);
    el.append(h('button', { class: 'small', onclick: () => { hide(); clearTimeout(el._t); action.run(); } }, action.label));
  }
  el.style.display = 'flex'; clearTimeout(el._t); el._t = setTimeout(hide, ms);
}
// A page that could not be drawn: what went wrong, in words for people, and a button to try again.
// A mistake in the app itself is not shown as program text (it goes to the browser's console).
function errorPage(e) {
  if (!e.offline && !e.api) console.error(e);
  const msg = e.offline || e.api ? e.message : t('Something went wrong.');
  return h('div', { class: 'empty errorpage' },
    h('div', { class: 'big', 'aria-hidden': 'true' }, e.offline ? '⚠' : '!'),
    h('p', {}, msg),
    h('button', { class: 'primary', onclick: () => render() }, t('Try again')));
}
// Titles marked watched or unwatched by hand. The message offers "Undo" when that gives every title back
// exactly what it had: not when one was half watched (its place in it, or which episodes, would be lost).
async function markWatched(list, watched, after = () => {}) {
  const seen = it => it.leafCount ? it.viewedLeafCount === it.leafCount : it.viewCount > 0;
  const clean = list.every(it => !it.viewOffset && (!it.leafCount || !it.viewedLeafCount || it.viewedLeafCount === it.leafCount));
  const before = list.map(it => ({ it, seen: seen(it), viewCount: it.viewCount, viewedLeafCount: it.viewedLeafCount, viewOffset: it.viewOffset }));
  await api('POST', `/:/${watched ? 'scrobble' : 'unscrobble'}?key=${list.map(it => it.id).join(',')}`);
  for (const it of list) {
    if (it.leafCount) it.viewedLeafCount = watched ? it.leafCount : 0;
    it.viewCount = watched ? Math.max(1, it.viewCount || 0) : 0; it.viewOffset = 0;
  }
  after();
  const msg = list.length === 1 ? (watched ? t('Marked as watched') : t('Marked as unwatched'))
    : watched ? t('{n} marked as watched', { n: list.length }) : t('{n} marked as unwatched', { n: list.length });
  // Only the titles that changed go back.
  const changed = before.filter(b => b.seen !== watched);
  toast(msg, 2600, clean && changed.length ? { label: t('Undo'), run: async () => {
    await api('POST', `/:/${watched ? 'unscrobble' : 'scrobble'}?key=${changed.map(b => b.it.id).join(',')}`);
    for (const b of changed) Object.assign(b.it, { viewCount: b.viewCount, viewedLeafCount: b.viewedLeafCount, viewOffset: b.viewOffset });
    after();
  } } : undefined);
}
// A button whose request failed says why, instead of doing nothing (cancelled page reads stay quiet).
window.addEventListener('unhandledrejection', e => { if (e.reason && e.reason.name !== 'AbortError') toast(t('Something went wrong: {msg}', { msg: e.reason.message || String(e.reason) }), 6000); });
// Picture, video and download addresses carry a short-lived ticket (GET /api/ticket) instead of the sign-in
// token, which would otherwise end up in proxy logs, the browser history and the download list.
// q: '' for pictures, 'item=<id>' for one item's video, subtitles, stills and theme, 'path=<path>' for one file to save.
let TK = {};
async function ticket(q = '') {
  const c = TK[q];
  if (c && c.expires - Date.now() / 1000 > 600) return c.ticket;
  // (Not through api(): a page change must not cancel it.)
  const r = await fetch('/api/ticket' + (q ? '?' + q : ''), { headers: { 'X-Token': S.token, Accept: 'application/json' } });
  if (!r.ok) throw new Error('HTTP ' + r.status);
  TK[q] = await r.json();
  return TK[q].ticket;
}
const withTicket = (p, tk) => p + (p.includes('?') ? '&' : '?') + 'ticket=' + encodeURIComponent(tk || '');
// A picture address: the picture ticket is fetched at start and renewed in the background.
const withTok = p => withTicket(p, TK[''] && TK[''].ticket);
setInterval(() => { if (S.me) ticket().catch(() => {}); }, 5 * 60 * 1000);
document.addEventListener('visibilitychange', () => { if (!document.hidden && S.me) ticket().catch(() => {}); });
// A ticket's end time (unix seconds) is its second field.
const ticketEnd = tk => Number(String(tk).split('.')[1]);
// The address with the current ticket in place of an older one; null when it has none or already has this one.
const freshTicket = (src, tk) => tk && /ticket=/.test(src) && !src.includes('ticket=' + encodeURIComponent(tk)) ? src.replace(/ticket=[^&]*/, 'ticket=' + encodeURIComponent(tk)) : null;
// A picture drawn earlier (lazy, below the fold) asks with a ticket that ran out meanwhile: retry once with the
// current one, before the picture's own error handler gives up on it. A second failure has the current ticket, so it stops.
document.addEventListener('error', e => { const src = e.target.tagName === 'IMG' && freshTicket(e.target.src, TK[''] && TK[''].ticket); if (src) { e.stopPropagation(); e.target.src = src; } }, true);
// A poster scaled down on the server to about w px wide (small pictures instead of the originals).
const smallTok = (p, w) => withTok(p + (p.includes('?') ? '&' : '?') + 'w=' + w);
const fmtNum = (n, digits) => Number(n).toLocaleString(I18N.lang, digits === undefined ? undefined : { minimumFractionDigits: digits, maximumFractionDigits: digits });
const fmtDur = ms => { if (!ms) return ''; const m = Math.round(ms / 60000); if (m < 1) return Math.max(1, Math.round(ms / 1000)) + ' s'; return m >= 60 ? t('{h} h {m} min', { h: Math.floor(m / 60), m: m % 60 }) : t('{m} min', { m }); };
// Where to go on from, in seconds; less than a second in counts as not started (no "Resume from 0:00").
const resumeAt = it => (it.viewOffset || 0) >= 1000 ? it.viewOffset / 1000 : 0;
const fmtTime = s => { s = Math.max(0, Math.floor(s)); const hh = Math.floor(s / 3600), mm = Math.floor(s / 60) % 60, ss = s % 60; return (hh ? hh + ':' + String(mm).padStart(2, '0') : mm) + ':' + String(ss).padStart(2, '0'); };
const fmtDate = sec => sec ? new Date(sec * 1000).toLocaleDateString(I18N.lang) : '';
const hour12 = () => { const v = store.get('timeFormat', 'auto'); return v === '12' ? true : v === '24' ? false : undefined; };
const fmtDateTime = sec => sec ? new Date(sec * 1000).toLocaleString(I18N.lang, { hour12: hour12() }) : '';
const fmtSize = b => b > 1e9 ? fmtNum(b / 1e9, 1) + ' GB' : fmtNum(b / 1e6, 0) + ' MB';
const CONTAINER_NAMES = { matroska: 'MKV', mov: 'MP4', mp4: 'MP4', mpegts: 'TS', avi: 'AVI', webm: 'WebM' };
const friendlyContainer = m => CONTAINER_NAMES[String(m.container || '').toLowerCase()] || String(m.container || '').toUpperCase();
// Every audio track on its own line: "German · AC3 · 5.1" (the first file's main track when the file lists none).
function audioTracks(m) {
  const auds = (m.streams || []).filter(s => s.kind === 'audio');
  if (!auds.length) return m.audioCodec ? [String(m.audioCodec).toUpperCase(), friendlyChannels(m.audioChannels)].filter(Boolean).join(' · ') : '–';
  return auds.map(a => h('div', {}, [a.language ? langName(a.language) : '', String(a.codec || '').toUpperCase(), friendlyChannels(a.channels), a.title && a.title !== a.language ? a.title : ''].filter(Boolean).join(' · ')));
}
const friendlyChannels = n => ({ 1: 'Mono', 2: t('Stereo'), 3: '2.1', 6: '5.1', 7: '6.1', 8: '7.1' }[n] || (n ? t('{n} channels', { n }) : ''));

async function api(method, path, body) {
  // A change makes what was fetched ahead outdated; a read of it uses the copy once.
  if (method !== 'GET') PREFETCH.clear();
  else if (PREFETCH.has(path)) {
    const p = PREFETCH.get(path); PREFETCH.delete(path);
    if (Date.now() - p.at < 30000) { const j = await p.json.catch(() => null); if (j) return j; }
  }
  const opt = { method, headers: { 'X-Token': S.token, Accept: 'application/json' } };
  if (body !== undefined) { opt.headers['Content-Type'] = 'application/json'; opt.body = JSON.stringify(body); }
  // A newer page cancels the reads of the one before (render() in pages.js), so a slow answer cannot draw over it.
  if (method === 'GET' && S.nav) opt.signal = S.nav.signal;
  let r;
  try { r = await fetch(path, opt); } catch (e) {
    if (e.name === 'AbortError') throw e;
    const x = new Error(t('No connection to the server. Check the network and try again.')); x.offline = true; throw x;
  }
  let j = null;
  try { j = await r.json(); } catch (e) { if (e.name === 'AbortError') throw e; /* not JSON */ }
  if (r.status === 401 && S.token && !path.startsWith('/api/login')) { logout(); throw new Error(t('Please sign in again')); }
  if (!r.ok) { const e = new Error((j && j.error) ? t(j.error) : t('The server answered with an error ({n}).', { n: r.status })); e.data = j; e.api = true; throw e; }
  // What a page was drawn from (render() compares it when going back).
  if (method === 'GET' && S.rec) S.rec.push(path + ' ' + JSON.stringify(j));
  return j;
}
const GET = p => api('GET', p);
// The mouse rests on a poster: its details (and a show's seasons) are fetched already, so the page opens at once.
const PREFETCH = new Map();
function prefetchItem(it) {
  const paths = ['/library/metadata/' + it.id];
  if (['show', 'season', 'artist', 'album'].includes(it.type)) paths.push(`/library/metadata/${it.id}/children`);
  for (const path of paths) {
    if (PREFETCH.has(path) || !S.token) continue;
    if (PREFETCH.size > 20) PREFETCH.delete(PREFETCH.keys().next().value);
    const json = fetch(path, { headers: { 'X-Token': S.token, Accept: 'application/json' } }).then(r => r.ok ? r.json() : null);
    json.catch(() => {});
    PREFETCH.set(path, { at: Date.now(), json });
  }
}
const meta = j => (j && j.MediaContainer && j.MediaContainer.Metadata) || [];

// ---------- sign in, first setup, invites
function logout() { S.token = ''; S.me = null; TK = {}; Q.list = []; saveQueue(); try { localStorage.removeItem('token'); sessionStorage.removeItem('token'); } catch (e) { /* none */ } location.hash = '#/login'; drawChrome(); render(); }

function authPage(title, intro, fields, button, submit) {
  const err = h('div', { class: 'err' });
  const inputs = {};
  const form = h('form', { class: 'center', method: 'post', action: '#', onsubmit: async e => {
    e.preventDefault(); err.textContent = '';
    const vals = {}; for (const [k, el] of Object.entries(inputs)) vals[k] = el.value;
    try { await submit(vals); } catch (x) { err.textContent = x.message; }
  } },
    h('h1', {}, title), intro ? h('p', { class: 'dim' }, intro) : null,
    fields.map(([k, label, type, val, ac]) => [h('label', { for: 'auth-' + k }, label),
      (inputs[k] = h('input', { type: type || 'text', value: val || '', id: 'auth-' + k, name: k, autocomplete: ac || 'off' }))]),
    h('div', { style: 'margin-top:18px' }, h('button', { class: 'primary', type: 'submit', style: 'width:100%' }, button)), err,
    Object.keys(I18N.packs).length > 1 ? h('div', { style: 'margin-top:14px;text-align:right' }, langSelect()) : null);
  set(form);
  const first = form.querySelector('input'); if (first) first.focus();
}
// Right after signing in: say so at once and keep the empty sign-in form from coming back while the app loads.
async function signedIn(token) {
  S.token = token; saveToken(token);
  S.signingIn = true;
  set(h('div', { class: 'center' }, h('h1', {}, t('Signed in')), h('p', { class: 'dim' }, t('Loading your libraries…'))));
  const after = S.after; S.after = '';
  location.hash = after || '#/';
  try { await boot(); } finally { S.signingIn = false; }
  if (S.me) toast(t('Signed in as {name}', { name: shownName(S.me) }));
}
async function loginPage() {
  authPage(t('Sign in'), S.status ? S.status.name : '', [['username', t('User name'), 'text', '', 'username'], ['password', t('Password'), 'password', '', 'current-password']], t('Sign in'),
    async v => {
      try { const r = await api('POST', '/api/login', { username: v.username, password: v.password, device: 'web' }); await signedIn(r.token); }
      catch (e) {
        if (!(e.data && e.data.needs_code)) throw e;
        // Two-step sign-in: the code from the authenticator app.
        authPage(t('Two-step sign-in'), t('Enter the 6-digit code from your authenticator app.'), [['code', t('Code'), 'text', '', 'one-time-code']], t('Sign in'),
          async w => { const r = await api('POST', '/api/login', { username: v.username, password: v.password, code: w.code, device: 'web' }); await signedIn(r.token); });
      }
    });
  const form = main().querySelector('form.center');
  // A mail server is set up: a link to a new password by e-mail.
  if (S.status && S.status.mail && form) form.querySelector('.err').after(h('div', { style: 'margin-top:12px' }, h('a', { href: '#/login', onclick: e => { e.preventDefault(); forgotPage(); } }, t('Forgot your password?'))));
  // Home profiles (only offered in the local network): a tap, and the PIN if there is one.
  let home;
  try { home = await api('GET', '/api/home'); } catch (e) { return; }
  if (!home.profiles.length || !form.isConnected) return;
  const pick = h('div', { class: 'profiles' }, home.profiles.map(p => h('button', { type: 'button', class: 'profile', title: p.username, onclick: () => pinPage(p) },
    h('span', { class: 'avatar' }, (shownName(p)[0] || '?').toUpperCase()),
    h('span', { class: 'pname' }, shownName(p), p.has_pin ? h('span', { class: 'dim', title: t('Needs a PIN') }, ' 🔒') : null))));
  const or = h('div', { class: 'or dim' }, t('or sign in with a password'));
  const firstLabel = form.querySelector('label');
  form.insertBefore(pick, firstLabel);
  form.insertBefore(or, firstLabel);
}
// "Forgot password": the link goes to the account's e-mail address. The answer is the same whether or not the name exists.
function forgotPage() {
  authPage(t('Forgot your password?'), t('Enter your user name or e-mail address. If your account has an e-mail address, a link to set a new password is sent to it.'),
    [['name', t('User name or e-mail address'), 'text', '', 'username']], t('Send link'),
    async v => {
      await api('POST', '/api/password/forgot', { name: v.name, lang: I18N.lang });
      set(h('div', { class: 'center' }, h('h1', {}, t('Look in your mailbox')),
        h('p', {}, t('If the account has an e-mail address, the link is on its way. It works for one hour.')),
        h('p', { class: 'dim' }, t('Nothing came? Look in the spam folder, or ask an admin to set a new password for you.')),
        h('a', { href: '#/login', onclick: e => { e.preventDefault(); loginPage(); } }, t('Back to sign-in'))));
    });
  main().querySelector('form.center').append(h('div', { style: 'margin-top:12px' }, h('a', { href: '#/login', onclick: e => { e.preventDefault(); loginPage(); } }, t('Back to sign-in'))));
}
// The link from the mail: pick the new password.
async function resetPage(code) {
  let info;
  try { info = await api('GET', '/api/password/reset/' + encodeURIComponent(code)); } catch (e) {
    return set(h('div', { class: 'center' }, h('h1', {}, t('New password')), h('p', { class: 'err' }, e.message),
      h('a', { href: '#/login', onclick: e => { e.preventDefault(); location.hash = '#/login'; forgotPage(); } }, t('Ask for a new link'))));
  }
  authPage(t('New password'), t('For {name} on {server}. Every device signed in with this account is signed out.', { name: info.username, server: info.server }),
    [['password', t('New password (at least 6 characters)'), 'password', '', 'new-password'], ['again', t('The same password again'), 'password', '', 'new-password']], t('Save password'),
    async v => {
      if (v.password !== v.again) throw new Error(t('The two passwords are not the same'));
      await api('POST', '/api/password/reset/' + encodeURIComponent(code), { password: v.password });
      // Signed in here too: that sign-in has just ended.
      if (S.me) logout(); else { history.replaceState(null, '', '#/login'); loginPage(); }
      toast(t('Password saved. Sign in with the new one.'), 6000);
    });
}
async function profileLogin(p, pin) {
  const r = await api('POST', `/api/home/${p.id}/login`, { pin, device: 'web' });
  await signedIn(r.token);
}
function pinPage(p) {
  if (!p.has_pin) return profileLogin(p, '').catch(e => toast(e.message));
  const err = h('div', { class: 'err' });
  const pin = h('input', { type: 'password', inputmode: 'numeric', pattern: '[0-9]*', maxlength: 8, autocomplete: 'off', class: 'pin', 'aria-label': t('PIN') });
  const form = h('form', { class: 'center', onsubmit: async e => {
    e.preventDefault(); err.textContent = '';
    try { await profileLogin(p, pin.value); } catch (x) { err.textContent = x.message; pin.value = ''; pin.focus(); }
  } },
    h('div', { class: 'profiles' }, h('div', { class: 'profile' }, h('span', { class: 'avatar' }, (shownName(p)[0] || '?').toUpperCase()), h('span', { class: 'pname' }, shownName(p)))),
    h('label', {}, t('PIN')), pin,
    h('div', { style: 'margin-top:18px' }, h('button', { class: 'primary', type: 'submit', style: 'width:100%' }, t('Sign in'))), err,
    h('div', { style: 'margin-top:14px' }, h('a', { href: '#/login', onclick: e => { e.preventDefault(); loginPage(); } }, t('Back to all profiles'))));
  set(form);
  pin.focus();
}
// Sign out here and go back to the profiles.
async function switchUser() {
  try { await api('POST', '/api/logout'); } catch (e) { /* signed out anyway */ }
  logout();
}
function setupPage() {
  authPage(t('Welcome'), t('Create the owner account for this server. You can invite other people afterwards.'),
    [['server_name', t('Server name'), 'text', 'Lichtspiel'], ['username', t('Your user name'), 'text', '', 'username'], ['password', t('Password (at least 6 characters)'), 'password', '', 'new-password']], t('Create account'),
    async v => { const r = await api('POST', '/api/setup', v); await signedIn(r.token); });
}
// Sign in a TV or app: it shows a short code, you type it here.
function linkForm(prefill, extra) {
  const code = h('input', { type: 'text', maxlength: 9, value: prefill || '', placeholder: 'ABCD', autocomplete: 'off', autocapitalize: 'characters', spellcheck: 'false', style: 'font-size:28px;letter-spacing:6px;text-transform:uppercase;text-align:center;width:9ch' });
  const msg = h('div', { class: 'dim', style: 'margin-top:10px' });
  const go = async () => {
    msg.className = 'dim'; msg.textContent = '';
    try { const r = await api('POST', '/api/link/confirm', { code: code.value }); code.value = ''; msg.className = 'ok'; msg.textContent = t('{device} is signed in as {name}.', { device: r.device || t('The device'), name: S.me.username }); }
    catch (e) { msg.className = 'err'; msg.textContent = e.message; }
  };
  code.addEventListener('keydown', e => { if (e.key === 'Enter') { e.preventDefault(); go(); } });
  return { code, nodes: [h('p', { class: 'dim' }, t('Open the app on your TV or phone and choose "Sign in with a code". Type the code it shows here.')),
    code, h('div', { class: 'actions', style: 'margin-top:14px' }, h('button', { class: 'primary', onclick: go }, t('Sign in device')), extra), msg] };
}
function linkPanel(prefill) { return h('div', { class: 'panel' }, h('h2', { style: 'margin-top:0' }, t('Sign in a device')), linkForm(prefill).nodes); }
// A box over the page you are on, not a page of its own.
function linkDialog(prefill) {
  const f = linkForm(prefill, h('button', { onclick: () => box.remove() }, t('Close')));
  const box = modal(t('Sign in a device'), f.nodes);
  f.code.focus();
  return box;
}
async function invitePage(code) {
  let info;
  try { info = await api('GET', '/api/invites/' + code); } catch (e) { return set(h('div', { class: 'center' }, h('h1', {}, t('Invite')), h('p', { class: 'err' }, e.message), h('a', { href: '#/login' }, t('Sign in instead')))); }
  authPage(t('Join {server}', { server: info.server }), t('{who} invited you{note}. Pick a user name and password.', { who: info.invited_by || t('Someone'), note: info.note ? ': ' + info.note : '' }),
    [['username', t('User name'), 'text', '', 'username'], ['email', t('Email (optional)'), 'email', info.email, 'email'], ['password', t('Password (at least 6 characters)'), 'password', '', 'new-password']], t('Join'),
    async v => { const r = await api('POST', `/api/invites/${code}/accept`, v); await signedIn(r.token); });
}

// ---------- chrome
async function boot() {
  I18N.applyStatic();
  // The four start-up questions go out together instead of one after the other.
  const [status, me, secs, filters, , panels] = await Promise.all([GET('/api/status'), GET('/api/me'), GET('/library/sections'), GET('/api/filters'), ticket(), GET('/api/plugins/panels')].map(p => p.catch(() => null)));
  S.pluginPages = panels ? panels.panels.filter(p => p.sidebar) : [];
  S.status = status;
  if (S.status) { $('#ver').textContent = `Lichtspiel ${S.status.version}${S.status.build ? ' · ' + S.status.build : ''}`; $('#ver').title = S.status.built_at ? t('Built {when}', { when: fmtDateTime(S.status.built_at) }) : ''; $('#sname').textContent = S.status.name; document.title = S.status.name === 'Lichtspiel' ? 'Lichtspiel' : `${S.status.name} · Lichtspiel`; }
  S.me = null;
  { // (Also without a token: networks set to need no sign-in are let in by the server.)
    if (me && secs && filters) { S.me = me; pullPrefs(); S.sections = secs.MediaContainer.Directory; S.filters = filters; }
  }
  drawChrome();
  // Settings → Web → remember the last page: open it again when the app is opened without an address.
  if (S.me && !location.hash.replace('#', '').replace(/^\/$/, '') && store.get('rememberTab', '0') === '1') {
    const last = store.get('lastPage', ''); if (last && !last.startsWith('#/play') && !last.startsWith('#/together')) location.hash = last;
  }
  render();
  ACT.done = -1; pollActivity();
  tgCheckInvites();
}
function drawChrome() {
  const who = $('#who');
  who.replaceChildren();
  const signed = !!S.me;
  $('#side').style.display = signed ? '' : 'none';
  $('#search').style.visibility = signed ? '' : 'hidden';
  $('#main').style.marginLeft = signed && innerWidth > 760 ? '220px' : '0';
  if (!signed) return;
  // The gear opens the settings; the colour scheme, switching user and signing out sit behind the name.
  const gear = iconBtn('gear', t('Settings'), () => { location.hash = '#/settings'; });
  const name = h('button', { class: 'link', 'aria-haspopup': 'menu', title: t('Appearance, switch user or sign out'), style: 'background:none;border:0;color:inherit;font:inherit;cursor:pointer;padding:4px 6px' }, shownName(S.me) + ' ▾');
  // The same choices as Settings → Web (kept with the account).
  const choose = (key, def, opts) => async () => opts.map(([v, l]) => ({ label: l, checked: store.get(key, def) === v, run: () => { store.set(key, v); window.applyTheme(); } }));
  name.onclick = () => openMenu(name, [
    { label: t('Appearance'), sub: choose('theme', 'system', [['system', t('System')], ['light', t('Light')], ['dark', t('Dark')], ['oled', t('OLED (pure black)')]]) },
    { label: t('Accent color'), sub: choose('accent', 'rose', [['rose', t('Rose')], ['amber', t('Amber')], ['blue', t('Blue')], ['green', t('Green')], ['red', t('Red')], ['violet', t('Violet')]]) },
    '-',
    { label: t('Switch user'), run: switchUser },
    { label: t('Sign in a device'), run: () => linkDialog() },
    { label: t('Sign out'), run: async () => { if (await sure(t('Sign out on this device?'), { ok: t('Sign out') })) switchUser(); } },
  ]);
  who.append(name, gear);
  document.querySelectorAll('#side .admin').forEach(e => (e.style.display = S.me.admin ? '' : 'none'));
  $('#npbtn').style.display = S.me.admin ? '' : 'none';
  drawQueueChip();
  drawNav();
  drawDlChip();
}
// The phone's bar at the bottom: its menu button opens the side bar over the page (and closes it again).
$('#tabmenu').addEventListener('click', e => { e.stopPropagation(); document.body.classList.toggle('nav-open'); });
function markNav(key) { NAV.cur = key; document.querySelectorAll('#side a, #tabbar a').forEach(a => a.classList.toggle('on', a.dataset.nav === key)); $('#npbtn').classList.toggle('on', key === 'dashboard'); }
let searchTimer;
// Typing searches as it goes. All of one search is a single entry in the history: the first letters open the
// search page, the next ones replace it, and emptying the field goes back to where the search began.
let searchFrom = null;
$('#q').addEventListener('input', e => { clearTimeout(searchTimer); searchTimer = setTimeout(() => {
  const v = e.target.value.trim(), on = location.hash.startsWith('#/search/');
  if (v && on) { BACK.pages.delete(BACK.cur); history.replaceState(history.state, '', '#/search/' + encodeURIComponent(v)); render(); }
  else if (v) { searchFrom = location.hash || '#/'; location.hash = '#/search/' + encodeURIComponent(v); }
  else if (on && searchFrom !== null) { searchFrom = null; history.back(); }
  else if (on) location.hash = '#/';
}, 300); });
window.addEventListener('hashchange', () => { if (!location.hash.startsWith('#/search/')) searchFrom = null; });

// ---------- cards
function posterCard(it, opts = {}) {
  const prog = it.viewOffset && it.duration ? Math.min(1, it.viewOffset / it.duration) : 0;
  let title = it.title, sub = it.year || '';
  if (it.editionTitle) sub = [it.year, it.editionTitle].filter(Boolean).join(' · ');
  if (it.type === 'episode') { title = it.grandparentTitle || it.title; sub = `${t('S{n}', { n: it.parentIndex || 0 })} ${t('E{n}', { n: it.index || 0 })} · ${it.title}`; }
  if (it.type === 'season') sub = it.parentTitle || '';
  if (it.type === 'show') sub = t('{n} episodes', { n: it.leafCount || 0 });
  if (it.type === 'season') title = seasonTitle(title);
  if (it.type === 'artist') sub = t('{n} songs', { n: it.leafCount || 0 });
  if (it.type === 'album') sub = [it.parentTitle, it.year, it.releaseType && it.releaseType !== 'album' ? releaseName(it.releaseType) : ''].filter(Boolean).join(' · ');
  if (it.type === 'track') sub = [it.grandparentTitle, it.parentTitle].filter(Boolean).join(' · ');
  const unseen = it.leafCount ? it.leafCount - (it.viewedLeafCount || 0) : 0;
  const img = it.type === 'episode' && it.grandparentThumb && !opts.episodeThumb ? it.grandparentThumb : it.thumb;
  const mark = store.get('watchedMark', 'all'); // Settings → Account → Watched indicator
  const seen = (mark === 'all' || (mark === 'movies') === (it.type === 'movie')) && mark !== 'none' && (it.viewCount > 0 || (it.leafCount && unseen === 0));
  // opts.pick(e, circle) = the library's multiple selection; true when it took the click.
  // The "…" menu redraws the card after a change; opts.onReplace learns of the new one.
  let card;
  const replace = fresh => { if (card.isConnected) card.replaceWith(fresh); card = fresh; if (opts.onReplace) opts.onReplace(fresh); };
  const menu = (anchor, touch) => {
    // After a change (match, refresh, unmatch, watched, undo …) the card is redrawn with what the server has now.
    itemMenu(it, anchor, async () => {
      try { Object.assign(it, meta(await GET('/library/metadata/' + it.id))[0]); } catch (err) { /* keep what we have */ }
      const fresh = posterCard(it, opts); replace(fresh);
    }, { cont: opts.cont, removed: () => card.remove(),
      // On a phone the menu also holds what the buttons on the poster do with a mouse.
      play: touch && isPlayable(it) && !it.missing ? () => play(it, (it.viewOffset || 0) / 1000) : null,
      select: touch && opts.pick ? () => opts.pick({ preventDefault() {}, shiftKey: false, ctrlKey: true }, true) : null });
  };
  // (The buttons sit inside the link: they do their own thing instead of opening the title.)
  const more = e => { e.preventDefault(); e.stopPropagation(); menu(e.currentTarget, false); };
  // The pencil (admins) edits the details right here; the card is redrawn with what was saved.
  const edit = async e => {
    e.preventDefault(); e.stopPropagation();
    editItem(await fullItem(it), async () => replace(posterCard(Object.assign(it, meta(await GET('/library/metadata/' + it.id))[0]), opts)));
  };
  // A real link: Tab and Enter reach it, Ctrl-click and the middle button open it in a new tab.
  card = h('a', { href: '#/item/' + it.id, class: 'card' + (seen ? ' watched' : '') + (it.missing ? ' missing' : '') + (['artist', 'album', 'track'].includes(it.type) ? ' music' : ''),
    onclick: e => {
      if (card._held || (opts.pick && opts.pick(e, false))) { e.preventDefault(); card._held = false; return; }
      // Where the poster is: the title's page lets it grow from here (see motion()). Not for a new tab.
      if (!e.ctrlKey && !e.metaKey && !e.shiftKey && !e.button) MOTION.from = { id: String(it.id), rect: card.querySelector('.poster').getBoundingClientRect(), at: Date.now() };
    },
    onpointerenter: e => { if (e.pointerType === 'mouse') setTimeout(() => { if (card.matches(':hover')) prefetchItem(it); }, 150); } },
    h('div', { class: 'poster', 'data-id': it.id }, img ? posterImg(img) : null,
      it.missing ? h('span', { class: 'gone', title: t('The file is gone from disk; the item is in the library’s trash'), 'aria-label': t('Unavailable') }, icon('trash')) : null,
      prog > 0 ? h('div', { class: 'bar' }, h('i', { style: `width:${prog * 100}%` })) : null,
      it.leafCount && unseen > 0 ? h('span', { class: 'badge' }, unseen) : null,
      seen ? h('span', { class: 'seen', title: t('Watched'), 'aria-label': t('Watched') }, icon('check')) : null,
      opts.pick ? h('span', { class: 'pick', title: t('Select'), onclick: e => { e.preventDefault(); e.stopPropagation(); opts.pick(e, true); } }) : null,
      isPlayable(it) && !it.missing ? h('button', { class: 'play', title: t('Play'), 'aria-label': t('Play'), onclick: e => { e.preventDefault(); e.stopPropagation(); play(it, (it.viewOffset || 0) / 1000); } }, icon('play')) : null,
      S.me.admin && it.type !== 'clip' ? h('button', { class: 'edit', title: t('Edit details'), 'aria-label': t('Edit details'), onclick: edit }, icon('edit')) : null,
      it.type === 'clip' ? null : h('button', { class: 'more', title: t('More'), 'aria-label': t('More'), onclick: more }, '⋯')),
    h('div', { class: 't', title }, title), h('div', { class: 's' }, (opts.caption && opts.caption(it)) || sub));
  // A finger held on the poster opens the menu (there is no hover for the buttons on it).
  if (it.type !== 'clip') longPress(card, () => menu(card.querySelector('.poster'), true));
  return card;
}
// A poster in the size it is shown: the browser picks the smallest copy that is sharp enough (the server
// makes them once; the slider in a library's bar can make the posters up to about twice as wide).
const POSTER_W = [240, 360, 600];
// In a list it fades in once loaded. With sizes (a title's own page): at once, no fade; the copy picked is
// mostly the one its card in the list already loaded, so the poster that grows into it is there right away.
function posterImg(src, sizes) {
  const at = w => withTok(src + (src.includes('?') ? '&' : '?') + 'w=' + w);
  return h('img', { src: at(360), srcset: POSTER_W.map(w => `${at(w)} ${w}w`).join(', '),
    sizes: sizes || `(max-width:760px) 34vw, ${Math.round(cardSize() * 1.4)}px`, loading: sizes ? null : 'lazy', alt: '',
    class: sizes ? null : 'fade', onload: sizes ? null : e => e.target.classList.add('in') });
}
// Holding a finger on el for half a second runs fn; the tap that ends it then opens nothing (el._held).
// The phone's own menu for links is left out there, as is selecting the text.
function longPress(el, fn) {
  let timer = 0, x = 0, y = 0, down = false;
  const stop = () => { clearTimeout(timer); down = false; };
  el.addEventListener('pointerdown', e => {
    el._held = false;
    if (e.pointerType !== 'touch') return;
    x = e.clientX; y = e.clientY; stop(); down = true;
    timer = setTimeout(() => { el._held = true; if (navigator.vibrate) navigator.vibrate(10); fn(); }, 500);
  });
  el.addEventListener('pointermove', e => { if (Math.abs(e.clientX - x) > 10 || Math.abs(e.clientY - y) > 10) stop(); });
  for (const ev of ['pointerup', 'pointercancel', 'pointerleave']) el.addEventListener(ev, stop);
  el.addEventListener('contextmenu', e => { if (el._held || down) e.preventDefault(); });
}
// Poster size in the grids (the slider in a library's bar), kept per browser.
const CARD = { min: 100, max: 300, def: 140 };
function cardSize(v) {
  if (v === undefined) { try { v = Number(localStorage.getItem('cardSize')); } catch (e) { /* storage blocked */ } }
  v = Math.max(CARD.min, Math.min(CARD.max, Number(v) || CARD.def));
  document.documentElement.style.setProperty('--card', v + 'px');
  return v;
}
cardSize();
function sizeSlider() {
  return h('label', { class: 'size', title: t('Poster size') }, h('span', { class: 'dim', style: 'font-size:11px' }, '▮'),
    h('input', { type: 'range', min: CARD.min, max: CARD.max, step: 10, value: cardSize(), 'aria-label': t('Poster size'),
      oninput: e => { const v = cardSize(e.target.value); store.set('cardSize', v); } }),
    h('span', { class: 'dim', style: 'font-size:18px' }, '▮'));
}
// Several items picked at once in a library: click the circle on a poster (or Ctrl-click),
// Shift-click picks everything in between. A bar at the bottom acts on all of them.
function pickGrid(items, cardOpts) {
  const picked = new Set(); let last = -1;
  const grid = h('div', { class: 'grid' });
  const count = h('b', {});
  const ids = () => [...picked].map(i => items[i].id);
  const playlistMenu = h('select', { onchange: async e => {
    const v = e.target.value; e.target.value = '';
    if (!v || !picked.size) return;
    if (v === 'new') { const name = await ask(t('Name of the new playlist')); if (name) { await api('POST', '/playlists', { title: name, kind: 'manual', ids: ids() }); toast(t('Added to {name}', { name })); } return; }
    const r = await api('POST', `/playlists/${v}/items`, { ids: ids() }); toast(r.added ? t('Added to playlist') : t('Already in that playlist'));
  } }, h('option', { value: '' }, t('+ Add to playlist')));
  let menuFilled = false;
  const fillMenu = async () => {
    if (menuFilled) return; menuFilled = true;
    const pls = meta(await GET('/playlists?kind=manual&owner=me&sort=title'));
    playlistMenu.append(...pls.map(p => h('option', { value: p.id }, p.title)), h('option', { value: 'new' }, t('New playlist…')));
  };
  const mark = watched => async () => {
    const idx = [...picked];
    // (Also after "Undo": the cards show what the titles have now.)
    const redraw = () => { for (const i of idx) if (cards[i]) { const fresh = card(i); cards[i].replaceWith(fresh); cards[i] = fresh; } draw(); };
    await markWatched(idx.map(i => items[i]), watched, redraw);
    clear();
  };
  const chosen = () => [...picked].sort((x, y) => x - y).map(i => items[i]);
  // Everything playable in the picked titles (episodes of a show, tracks of an album …), in order.
  const leaves = async () => (await Promise.all(chosen().map(leavesOf))).flat();
  const playPicked = shuffle => async () => {
    let list = await leaves();
    if (!list.length) return toast(t('Nothing to add'));
    if (shuffle) list = smartShuffle(list);
    const [first, ...rest] = list;
    play(first, !shuffle && first.viewOffset ? first.viewOffset / 1000 : 0, rest);
  };
  const eachItem = async (path, msg) => { const list = chosen(); for (const it of list) await api('POST', path(it)); toast(msg(list.length)); };
  const moreMenu = e => {
    const admin = S.me.admin, one = picked.size === 1;
    openMenu(e.currentTarget, [
      { label: t('Shuffle'), run: playPicked(true) },
      { label: t('Add to queue'), run: async () => { for (const it of chosen()) await addToQueue(it); } },
      S.me.can_download ? { label: t('Download…'), run: () => downloadDialog(ids().map(id => items.find(x => x.id === id))) } : null,
      admin ? '-' : null,
      admin && picked.size > 1 && chosen().every(x => x.type === chosen()[0].type && (x.type === 'movie' || x.type === 'episode')) ? { label: t('Merge into one…'), run: () => mergeDialog(chosen(), () => { clear(); render(); }) } : null,
      admin ? { label: t('Refresh metadata'), run: () => eachItem(it => `/library/metadata/${it.id}/refresh`, n => t('Downloading details for {n} titles', { n })) } : null,
      admin ? { label: t('Analyze'), run: async () => { await eachItem(it => `/library/metadata/${it.id}/analyze`, () => t('Analyzing — progress is shown at the top')); pollActivity(); } } : null,
      admin && one ? { label: t('Optimize…'), run: () => optimizeDialog(chosen()[0]) } : null,
      '-',
      { label: t('Select all'), run: () => { items.forEach((it, i) => { if (it) picked.add(i); }); draw(); } },
    ], null, 'left');
  };
  const bar = h('div', { class: 'pickbar' }, count,
    h('button', { class: 'small', onclick: () => clear() }, t('Clear selection')),
    h('button', { class: 'small primary', onclick: playPicked(false) }, t('▶ Play')),
    h('button', { class: 'small', onclick: async () => {
      const on = !(cardOpts && cardOpts.watchlistPage), idx = [...picked];
      const r = await api('POST', '/api/watchlist', { ids: ids(), watchlist: on });
      toast(on ? t('{n} added to your watchlist', { n: r.changed }) : t('{n} removed from your watchlist', { n: idx.length }));
      if (!on) return render();
      for (const i of idx) items[i].watchlist = true;
      clear();
    } }, cardOpts && cardOpts.watchlistPage ? t('Remove from watchlist') : t('+ Watchlist')),
    h('button', { class: 'small', onclick: mark(true) }, t('Mark watched')),
    h('button', { class: 'small', onclick: mark(false) }, t('Mark unwatched')),
    playlistMenu,
    S.me.admin ? h('button', { class: 'small', onclick: async () => {
      if (picked.size === 1) return editItem(await fullItem(chosen()[0]));
      // Several titles: one dialog for all; afterwards the cards show what was saved.
      bulkEdit(chosen(), async () => {
        for (const i of [...picked]) {
          try { Object.assign(items[i], meta(await GET('/library/metadata/' + items[i].id))[0]); } catch (err) { /* keep what we have */ }
          if (cards[i]) { const fresh = card(i); cards[i].replaceWith(fresh); cards[i] = fresh; }
        }
        clear();
      });
    } }, t('Edit')) : null,
    h('button', { class: 'small', title: t('More'), 'aria-label': t('More'), onclick: moreMenu }, '⋯'));
  const draw = () => {
    cards.forEach((c, i) => c.classList.toggle('picked', picked.has(i)));
    grid.classList.toggle('picking', picked.size > 0);
    count.textContent = picked.size === 1 ? t('1 selected') : t('{n} selected', { n: picked.size });
    bar.classList.toggle('show', picked.size > 0);
    if (picked.size) fillMenu();
  };
  const clear = () => { picked.clear(); last = -1; draw(); };
  const toggle = (i, e) => {
    if (e.shiftKey && last >= 0) { for (let k = Math.min(last, i); k <= Math.max(last, i); k++) if (items[k]) picked.add(k); }
    else if (picked.has(i)) picked.delete(i); else picked.add(i);
    last = i; draw();
  };
  const card0 = i => posterCard(items[i], { ...cardOpts, onReplace: el => { el.dataset.i = i; cards[i] = el; draw(); }, pick: (e, circle) => {
    // A plain click on the poster always opens the item, even while others are selected; the circle, Ctrl and Shift select.
    // On a touch screen a tap selects while something is selected (there is no Ctrl, and the circles are small).
    const touchPicking = picked.size > 0 && matchMedia('(hover: none)').matches;
    if (!circle && !touchPicking && !e.ctrlKey && !e.metaKey && !e.shiftKey) return false;
    e.preventDefault(); toggle(i, e); return true;
  } });
  const card = i => { const c = card0(i); c.dataset.i = i; c.classList.toggle('picked', picked.has(i)); return c; };
  const cards = []; // the cards of loaded titles (made when first shown; kept while scrolled away)
  // The blocks above and below the cards of a big library (see below).
  const top = h('div', { class: 'grid-rest', 'aria-hidden': 'true' }), bottom = h('div', { class: 'grid-rest', 'aria-hidden': 'true' });
  // More items arrive while scrolling.
  const put = el => (bottom.isConnected ? bottom.before(el) : grid.append(el));
  grid._add = list => { for (const it of list) { const i = items.length; items.push(it); cards.push(card(i)); put(cards[i]); } };
  const firstItems = items.splice(0); grid._add(firstItems);
  grid._clear = () => { if (!picked.size) return false; clear(); return true; };
  // A library with titles not loaded yet (items[i] is null until _put fills it). A big one (more than ALL titles)
  // has only the cards around the screen: from lo to hi, whole rows, with a block above and below as tall as the
  // rows they stand for, so the scroll bar and a saved scroll position are right. Scrolling (or a jump) moves
  // that window; the cards of loaded titles are kept and put back, empty cards are made anew.
  // (80,000 empty cards at once froze the page for seconds, most of it laying them out.)
  // _onNew(cards) learns of each new empty card, _onGone(cards) of each one taken out.
  const ALL = 2000, MARGIN = 1500;
  const ph = new Map(); // i -> the empty card shown for it
  let want = 0, lo = 0, hi = 0, cols = 1, rowH = 200, gap = 0, width = 0;
  const at = i => cards[i] || ph.get(i);
  const columns = () => Math.max(1, getComputedStyle(grid).gridTemplateColumns.split(' ').filter(Boolean).length);
  // A row's height with its gap: from two rows on the screen, else from one card.
  const measure = () => {
    gap = parseFloat(getComputedStyle(grid).rowGap) || 0;
    const a = at(lo), b = hi - lo > cols ? at(lo + cols) : null;
    if (a && b && b.offsetTop > a.offsetTop) rowH = b.offsetTop - a.offsetTop;
    else if (a && a.offsetHeight) rowH = a.offsetHeight + gap;
  };
  const sizeBlock = (el, rows) => { el.style.display = rows > 0 ? '' : 'none'; el.style.height = rows > 0 ? (rows * rowH - gap) + 'px' : ''; };
  const sizeBlocks = () => { sizeBlock(top, lo / cols); sizeBlock(bottom, Math.ceil(want / cols) - Math.ceil(hi / cols)); };
  // The cards for the screen with its top `y` px below the grid's top, and `margin` px more above and below.
  const range = (y, margin) => {
    if (want <= ALL) return [0, want];
    const rows = Math.ceil(want / cols);
    const r0 = Math.min(rows - 1, Math.max(0, Math.floor((y - margin) / rowH)));
    const r1 = Math.max(r0 + 1, Math.ceil((y + innerHeight + margin) / rowH));
    return [r0 * cols, Math.min(want, r1 * cols)];
  };
  const show = (nlo, nhi) => {
    const gone = [], made = [];
    for (let i = lo; i < hi; i++) if (i < nlo || i >= nhi) { const e = at(i); if (!e) continue; e.remove(); if (ph.get(i) === e) { ph.delete(i); gone.push(e); } }
    const get = i => {
      if (cards[i]) return cards[i];
      if (items[i]) return (cards[i] = card(i));
      const e = h('div', { class: 'card ph' + (music ? ' music' : ''), 'data-i': i }, h('div', { class: 'poster' }), h('div', { class: 't' }, '\u00a0'), h('div', { class: 's' }, '\u00a0'));
      ph.set(i, e); made.push(e); return e;
    };
    const keepLo = Math.max(lo, nlo), keepHi = Math.min(hi, nhi);
    const front = document.createDocumentFragment(), back = document.createDocumentFragment();
    if (keepLo < keepHi) { for (let i = nlo; i < keepLo; i++) front.append(get(i)); for (let i = keepHi; i < nhi; i++) back.append(get(i)); }
    else for (let i = nlo; i < nhi; i++) back.append(get(i));
    top.after(front); bottom.before(back);
    lo = nlo; hi = nhi; sizeBlocks();
    if (gone.length && grid._onGone) grid._onGone(gone);
    if (made.length && grid._onNew) grid._onNew(made);
  };
  let music = false;
  // Move the window when the screen comes near its edge (half the margin), so not every scroll step changes it.
  const sync = () => {
    if (!grid.isConnected || !want) return;
    const y = -grid.getBoundingClientRect().top;
    const [alo, ahi] = range(y, MARGIN / 2);
    if (lo <= alo && hi >= ahi) return;
    show(...range(y, MARGIN));
  };
  // Another poster size or window width: other rows. The title at the top of the screen stays there.
  const relayout = () => {
    const y = -grid.getBoundingClientRect().top, inside = y > 0;
    const first = inside ? Math.floor(y / rowH) * cols : 0, into = inside ? Math.min(y - Math.floor(y / rowH) * rowH, rowH) : 0;
    cols = columns(); width = grid.clientWidth;
    measure();
    const ny = Math.floor(first / cols) * rowH + into;
    show(...range(ny, MARGIN)); measure(); sizeBlocks();
    if (inside) window.scrollTo(0, window.scrollY + grid.getBoundingClientRect().top + Math.floor(first / cols) * rowH + into);
    sync();
  };
  let ticking = 0;
  const onScroll = () => {
    if (ticking) return;
    ticking = requestAnimationFrame(() => { ticking = 0; if (!grid.isConnected) { window.removeEventListener('scroll', onScroll); return; } sync(); });
  };
  grid._reserve = n => {
    want = items.length + n;
    music = !!(cards[0] && cards[0].classList.contains('music'));
    grid.prepend(top); grid.append(bottom);
    lo = 0; hi = items.length; cols = columns(); width = grid.clientWidth; measure();
    if (want <= ALL) show(0, want); else { sizeBlocks(); sync(); }
    window.addEventListener('scroll', onScroll, { passive: true });
    const ro = new ResizeObserver(() => {
      if (!grid.isConnected) { ro.disconnect(); return; }
      if (grid.clientWidth !== width || columns() !== cols) relayout();
    });
    ro.observe(grid);
  };
  // Where title i's card is (page coordinates), also while it is not on the page; and its card, if shown.
  grid._top = i => { const e = i >= lo && i < hi ? at(i) : null; return window.scrollY + (e ? e.getBoundingClientRect().top : grid.getBoundingClientRect().top + Math.floor(i / cols) * rowH); };
  grid._card = i => (i >= lo && i < hi ? at(i) || null : null);
  grid._sync = sync;
  grid._put = (start, list) => list.forEach((it, k) => {
    const i = start + k;
    if (i >= want || items[i]) return;
    items[i] = it;
    const old = ph.get(i);
    if (old) { ph.delete(i); const c = (cards[i] = card(i)); old.replaceWith(c); }
  });
  // The bar sits outside the page, so it goes when the page changes.
  document.querySelectorAll('.pickbar').forEach(b => b.remove());
  document.body.append(bar);
  return grid;
}
window.addEventListener('hashchange', () => { if (location.hash.length > 2) store.set('lastPage', location.hash); });
window.addEventListener('hashchange', () => document.querySelectorAll('.pickbar').forEach(b => b.remove()));
document.addEventListener('keydown', e => {
  if (e.key !== 'Escape' || P.it || document.querySelector('.modal')) return;
  const g = document.querySelector('.grid.picking'); if (g && g._clear) g._clear();
});
// Season names the scanner makes ("Season 2", "Specials") follow the language; others stay.
const seasonTitle = s => /^(Season \d+|Specials)$/.test(s || '') ? t(s) : s;
// A sideways row without a scrollbar: arrows on the left and right (shown on
// hover when there is more to see) scroll it by most of its width.
function scroller(cls, kids) {
  const row = h('div', { class: cls }, kids);
  const go = d => row.scrollBy({ left: d * row.clientWidth * 0.85 });
  const l = h('button', { class: 'arrow l', title: t('Back'), onclick: () => go(-1) }, '‹');
  const r = h('button', { class: 'arrow r', title: t('More'), onclick: () => go(1) }, '›');
  const upd = () => { l.classList.toggle('show', row.scrollLeft > 4); r.classList.toggle('show', row.scrollLeft + row.clientWidth < row.scrollWidth - 4); };
  row.addEventListener('scroll', upd, { passive: true });
  new ResizeObserver(upd).observe(row);
  return h('div', { class: 'rowwrap' }, l, row, r);
}
// Row titles come from the server in English ("Continue Watching", "More by …").
const row = (title, items) => items.length ? [h('h2', {}, t(title)), scroller('row', items.map(i => posterCard(i, { cont: title === 'Continue Watching' })))] : null;

// ---------- home, search, browse
// Grey placeholders while a list loads, only when it takes a moment (a quick answer would just flicker).
// skeletonSoon(draw) draws them after 150 ms; the function it returns stops that.
// Anything drawn meanwhile (the page, an error, another page) calls them off.
function skeletonSoon(draw) {
  clearTimeout(S.skel);
  const off = S.offscreen;
  S.skel = setTimeout(() => { S.skel = 0; if (S.offscreen === off) draw(); }, 150);
  return () => clearTimeout(S.skel);
}
const skeletonGrid = n => h('div', { class: 'grid' }, Array.from({ length: n }, () => h('div', { class: 'card ph' }, h('div', { class: 'poster skel' }), h('div', { class: 'skel skel-t' }))));
// Grey placeholders while a page's rows are still on their way.
const skeletonRows = n => Array.from({ length: n }, () => [h('div', { class: 'skel skel-h' }),
  h('div', { class: 'row' }, Array.from({ length: 10 }, () => h('div', { class: 'card' }, h('div', { class: 'poster skel' }), h('div', { class: 'skel skel-t' }))))]);
// ---------- episodes as cards (a season's page, and under the tabs of a show's page)
// The picture plays (the episodes after it queued), the title opens the episode, "⋯" has the rest
// (watched, edit …); after a change there, changed() draws the page again.
function episodeCards(eps, changed) {
  return h('div', { class: 'eps' }, eps.map((e, i) => {
    const at = resumeAt(e), prog = at && e.duration ? Math.min(1, e.viewOffset / e.duration) : 0, seen = e.viewCount > 0;
    const go = () => play(e, at, eps.slice(i + 1));
    return h('div', { class: 'card ep' + (seen ? ' watched' : '') + (e.missing ? ' missing' : '') },
      h('div', { class: 'poster', 'data-id': e.id, onclick: e.missing ? null : go },
        e.thumb ? posterImg(e.thumb, '(max-width:760px) 92vw, 320px') : null,
        e.missing ? h('span', { class: 'gone', title: t('The file is gone from disk; the item is in the library’s trash'), 'aria-label': t('Unavailable') }, icon('trash')) : null,
        prog ? h('div', { class: 'bar' }, h('i', { style: `width:${prog * 100}%` })) : null,
        seen ? h('span', { class: 'seen', title: t('Watched'), 'aria-label': t('Watched') }, icon('check')) : null,
        e.missing ? null : h('button', { class: 'play', title: t('Play'), 'aria-label': t('Play'), onclick: ev => { ev.stopPropagation(); go(); } }, icon('play')),
        h('button', { class: 'more', title: t('More'), 'aria-label': t('More'), onclick: ev => { ev.stopPropagation(); itemMenu(e, ev.currentTarget, changed); } }, '⋯')),
      h('a', { class: 't', href: '#/item/' + e.id, title: e.title }, e.index ? `${e.index}. ${e.title}` : e.title),
      h('div', { class: 's' }, [e.missing ? t('Unavailable') : '', fmtDur(e.duration), at ? t('stopped at {time}', { time: fmtTime(at) }) : ''].filter(Boolean).join(' · ')),
      e.summary ? h('div', { class: 'ep-plot' }, e.summary) : null);
  }));
}
// The season a show's page opens on: the one last chosen there, else the first with something not watched yet.
const SEASON_PICK = new Map();
function firstSeason(showId, seasons) {
  const kept = seasons.findIndex(s => String(s.id) === SEASON_PICK.get(String(showId)));
  if (kept >= 0) return kept;
  const open = seasons.findIndex(s => s.leafCount && (s.viewedLeafCount || 0) < s.leafCount && s.index !== 0);
  return open >= 0 ? open : 0;
}
// A show's seasons as tabs, the chosen one's episodes below (the first already loaded with the page).
function seasonTabs(showId, seasons, at, firstEps, changed) {
  const cache = new Map([[String(seasons[at].id), firstEps]]);
  let cur = at;
  const box = h('div', { class: 'season-eps' }, episodeCards(firstEps, changed));
  // A season's menu (watched, edit …), as its poster had before: a right click or a held finger on its tab, or the
  // chosen tab clicked again, which its "⋯" hints at (it moves along with the choice).
  const more = h('span', { class: 'more', title: t('More'), 'aria-hidden': 'true' }, '⋯');
  const tabs = seasons.length > 1 ? h('div', { class: 'stabs', role: 'tablist' }, seasons.map((s, k) => {
    const left = s.leafCount ? s.leafCount - (s.viewedLeafCount || 0) : 0;
    const tab = h('button', { type: 'button', role: 'tab', class: k === at ? 'on' : '', 'aria-selected': String(k === at), 'aria-haspopup': 'menu',
      oncontextmenu: ev => { ev.preventDefault(); itemMenu(s, tab, changed); }, onclick: async ev => {
      const b = ev.currentTarget;
      if (b._held) return; // the held finger opened the menu
      if (k === cur) { itemMenu(s, b, changed); return; } // the chosen tab (and its "⋯"): its menu, also by keyboard and remote
      tabs.querySelectorAll('button').forEach(x => { x.classList.toggle('on', x === b); x.setAttribute('aria-selected', String(x === b)); });
      b.append(more);
      SEASON_PICK.set(String(showId), String(s.id)); cur = k;
      let eps = cache.get(String(s.id));
      if (!eps) {
        box.classList.add('loading');
        try { eps = meta(await GET(`/library/metadata/${s.id}/children`)); cache.set(String(s.id), eps); }
        catch (e) { box.classList.remove('loading'); if (e.name !== 'AbortError') toast(e.message); return; }
      }
      if (!b.classList.contains('on')) return; // another tab was chosen meanwhile
      box.classList.remove('loading');
      box.replaceChildren(episodeCards(eps, changed));
      if (!calm()) [...box.querySelectorAll('.card')].slice(0, 12).forEach((c, i) => fadeIn(c, i * 35, 'translateY(10px)'));
    } }, seasonTitle(s.title), left && left < s.leafCount ? h('small', {}, left) : null, k === at ? more : null);
    longPress(tab, () => itemMenu(s, tab, changed));
    return tab;
  })) : null;
  // Only one season on disk: no tabs and no second "⋯" (the show's own does the same); the heading says these are the
  // show's episodes, so the page is not taken for that season's.
  if (!tabs) return [h('h2', {}, t('Episodes'), ' ', h('span', { class: 'dim', style: 'font-weight:normal' }, '· ' + seasonTitle(seasons[0].title))), box];
  return [h('h2', {}, t('Seasons')), h('div', { class: 'stabs-row' }, tabs), box];
}
// A show or season: the first episode not watched yet (else the first), the rest of them queued after it.
async function playNextEpisode(id) {
  const l = meta(await GET(`/library/metadata/${id}/allLeaves`));
  const e = l.find(x => !x.viewCount) || l[0];
  if (e) play(e, e.viewOffset ? e.viewOffset / 1000 : 0, l.slice(l.indexOf(e) + 1));
}
// ---------- the home page's banner: a few titles in turn, big, over their backdrop
// What it shows: where you left off first, then what is new; one title per show; at most five.
function spotItems(hubs) {
  const of = id => ((hubs.find(hb => hb.hubIdentifier === id) || {}).Metadata || []);
  const pool = [...of('continueWatching').slice(0, 2), ...of('recentlyAddedMovies').slice(0, 4), ...of('recentlyAddedEpisodes').slice(0, 3)];
  // Nothing new and nothing started: whatever the other rows hold.
  if (!pool.length) for (const hb of hubs) pool.push(...(hb.Metadata || []).slice(0, 3));
  const seen = new Set(), out = [];
  for (const it of pool) {
    if (!it || it.missing || !['movie', 'show', 'season', 'episode'].includes(it.type)) continue;
    const key = it.grandparentRatingKey || (it.type === 'season' ? it.parentRatingKey : null) || it.id;
    if (seen.has(String(key))) continue;
    seen.add(String(key));
    const hub = hubs.find(hb => (hb.Metadata || []).includes(it));
    out.push({ it, label: hub ? t(hub.title) : '' });
    if (out.length >= 5) break;
  }
  return out;
}
// Each title stays about nine seconds (the bar in its dot shows how long; the mouse on the banner holds it),
// a dot or a swipe picks another. Nothing turns by itself when the system asks for less motion.
function spotlight(list) {
  if (!list.length) return null;
  let cur = -1;
  const bg = h('div', { class: 'spot-bg' }), body = h('div', { class: 'spot-body' }), dots = h('div', { class: 'spot-dots' });
  const imgs = [];
  const backdrop = i => {
    if (imgs[i] || !list[i]) return;
    const it = list[i].it, src = it.grandparentRatingKey || (it.type === 'season' ? it.parentRatingKey : null) || it.id;
    imgs[i] = h('img', { src: withTok(`/library/metadata/${src}/art?v=${it.updatedAt || 0}`), alt: '', onload: e => e.target.classList.add('ok') });
    bg.append(imgs[i]);
  };
  const content = ({ it, label }) => {
    const show = it.type === 'episode' || it.type === 'season';
    const title = it.type === 'episode' ? it.grandparentTitle : it.type === 'season' ? it.parentTitle : it.title;
    const sub = it.type === 'episode' ? `${t('S{n}', { n: it.parentIndex || 0 })} ${t('E{n}', { n: it.index || 0 })} · ${it.title}` : it.type === 'season' ? seasonTitle(it.title) : '';
    const facts = [it.year, it.duration && it.type !== 'show' ? fmtDur(it.duration) : '', ...(it.genres || []).slice(0, 2)].filter(Boolean).join(' · ');
    const playable = isPlayable(it), at = resumeAt(it);
    const head = h('h2', { class: 'spot-title' }, title);
    // The title's logo, when it has one, instead of the name (details asked for once, also ready for a click).
    if (!show) {
      prefetchItem(it);
      const p = PREFETCH.get('/library/metadata/' + it.id);
      if (p) p.json.then(j => {
        const logo = j && j.MediaContainer && (j.MediaContainer.Metadata || [])[0] && j.MediaContainer.Metadata[0].clearLogo;
        if (logo && head.isConnected) head.replaceChildren(h('img', { src: withTok(logo), alt: title, onerror: e => e.target.replaceWith(title) }));
      }).catch(() => {});
    }
    return [label ? h('div', { class: 'spot-label' }, label) : null, head,
      sub || facts ? h('div', { class: 'spot-facts' }, [sub, facts].filter(Boolean).join(' · ')) : null,
      it.summary ? h('p', { class: 'spot-sum' }, it.summary) : null,
      h('div', { class: 'spot-act' },
        playable ? h('button', { class: 'primary big', onclick: () => play(it, at) }, at ? t('▶ Resume from {time}', { time: fmtTime(at) }) : t('▶ Play'))
          : (it.type === 'show' || it.type === 'season') && it.leafCount ? h('button', { class: 'primary big', onclick: () => playNextEpisode(it.id) }, t('▶ Play next episode')) : null,
        h('a', { class: 'spot-more', href: '#/item/' + it.id }, t('More info')))].filter(Boolean);
  };
  const go = i => {
    i = (i + list.length) % list.length;
    if (i === cur) return;
    cur = i; backdrop(i); backdrop(i + 1);
    imgs.forEach((im, k) => im && im.classList.toggle('on', k === i));
    body.replaceChildren(...content(list[i]));
    body.classList.remove('anim'); void body.offsetWidth; body.classList.add('anim');
    [...dots.children].forEach((d, k) => { d.classList.toggle('on', k === i); d.setAttribute('aria-current', String(k === i)); });
  };
  if (list.length > 1) {
    // (The button is a bigger, invisible place to tap; the dot is the <i> in it.)
    dots.append(...list.map(({ it }, k) => h('button', { type: 'button', title: it.grandparentTitle || it.title, 'aria-label': it.grandparentTitle || it.title, onclick: () => go(k) }, h('i'))));
    // The running bar in the dot ends → the next title.
    dots.addEventListener('animationend', e => { if (e.animationName === 'spot-run' && e.target.closest('button.on')) go(cur + 1); });
  }
  const el = h('section', { class: 'spot' + (list.length > 1 && !calm() ? ' turning' : ''), 'aria-roledescription': 'carousel' }, bg, body, dots);
  let x0 = null;
  el.addEventListener('pointerdown', e => { if (e.pointerType === 'touch') x0 = e.clientX; });
  el.addEventListener('pointerup', e => { if (x0 === null) return; const dx = e.clientX - x0; x0 = null; if (Math.abs(dx) > 50 && list.length > 1) go(cur + (dx < 0 ? 1 : -1)); });
  el.addEventListener('pointercancel', () => { x0 = null; });
  go(0);
  return el;
}
async function homePage() {
  markNav('home');
  // Answer the click at once; the rows come when the server has them.
  // (Not when the home page is drawn again over itself: the banner there may stay, see below.)
  if (S.sections.length && !main().querySelector('.spot:not(.skel)')) set(S.hadSpot ? h('div', { class: 'spot skel' }) : h('h1', {}, t('Home')), skeletonRows(2));
  // "Coming soon" comes from its own address (/hubs is also read by Plex apps) and joins the rows after the watchlist.
  const [home, coming] = await Promise.all([GET('/hubs'), GET('/api/upcoming?days=30').catch(() => ({ entries: [] }))]);
  const hubs = home.MediaContainer.Hub;
  if (coming.entries.length) {
    const at = hubs.findIndex(hb => !['continueWatching', 'watchlist'].includes(hb.hubIdentifier));
    hubs.splice(at < 0 ? hubs.length : at, 0, { hubIdentifier: 'upcoming', title: 'Coming Soon', Metadata: coming.entries, coming: true });
  }
  if (!S.sections.length) {
    return set(h('h1', {}, t('Welcome')), h('div', { class: 'empty' },
      S.me.admin ? [h('p', {}, t('No libraries yet. Add your movie and TV show folders to get started.')), h('button', { class: 'primary', onclick: () => (location.hash = '#/libraries') }, t('Add a library'))]
        : t('The owner has not shared any libraries with you yet.')));
  }
  // Drawn again in place (new details arrived while a library is scanned, every few seconds): the same
  // titles keep the banner as it is, on the title it shows, instead of starting over at the first one.
  const lay = homeLayout();
  const picks = lay.banner ? spotItems(hubs) : [], key = picks.map(({ it }) => [it.id, it.viewOffset || 0, it.updatedAt || 0].join(':')).join(' ');
  const old = !S.offscreen && main().querySelector('.spot:not(.skel)');
  const spot = old && old._key === key ? old : spotlight(picks);
  if (spot) spot._key = key;
  S.hadSpot = !!spot;
  const shown = orderedHubs(hubs, lay).filter(hb => !lay.hidden.includes(hb.hubIdentifier));
  const rows = [shown.length ? shown.map(hb => hb.coming ? comingRow(hb) : row(hb.title, hb.Metadata)) : h('div', { class: 'empty' }, hubs.length ? t('All rows are hidden.') : t('Nothing here yet. Libraries are still being scanned, or they are empty.')),
    hubs.length ? h('div', { class: 'home-edit' }, h('button', { class: 'small', onclick: () => editHome(hubs) }, t('Customize home'))) : null];
  // The kept banner stays in the page (taken out and put back, its running bar would start over): only the rows change.
  if (spot && spot === old) {
    clearTimeout(S.skel);
    for (const c of [...main().children]) if (c !== old) c.remove();
    main().append(...[rows].flat(9).filter(Boolean));
    return;
  }
  set(spot || h('h1', {}, t('Home')), rows);
}
// ---------- the home page as each person wants it: which rows, in which order, the banner or not
// (kept with the account, so it is the same in every browser). Rows not known yet come at the end.
function homeLayout() {
  let v = {}; try { v = JSON.parse(store.get('homeLayout', '{}')) || {}; } catch (e) { /* none */ }
  return { order: Array.isArray(v.order) ? v.order : [], hidden: Array.isArray(v.hidden) ? v.hidden : [], banner: v.banner !== false };
}
function orderedHubs(hubs, lay) {
  const at = id => { const i = lay.order.indexOf(id); return i < 0 ? lay.order.length : i; };
  return hubs.map((hb, i) => [hb, i]).sort((a, b) => at(a[0].hubIdentifier) - at(b[0].hubIdentifier) || a[1] - b[1]).map(x => x[0]);
}
function editHome(hubs) {
  const lay = homeLayout();
  const list = orderedHubs(hubs, lay).map(hb => ({ id: hb.hubIdentifier, title: t(hb.title), on: !lay.hidden.includes(hb.hubIdentifier) }));
  let banner = lay.banner;
  const box = h('div', { class: 'home-rows' });
  const draw = () => box.replaceChildren(...list.map((r, i) => h('div', { class: 'li' },
    h('label', { class: 'grow' }, h('input', { type: 'checkbox', checked: r.on, onchange: e => { r.on = e.target.checked; } }), ' ', r.title),
    h('button', { class: 'small', title: t('Move up'), 'aria-label': t('Move up'), disabled: i === 0, onclick: () => { [list[i - 1], list[i]] = [list[i], list[i - 1]]; draw(); } }, '▲'),
    h('button', { class: 'small', title: t('Move down'), 'aria-label': t('Move down'), disabled: i === list.length - 1, onclick: () => { [list[i + 1], list[i]] = [list[i], list[i + 1]]; draw(); } }, '▼'))));
  draw();
  const save = v => { store.set('homeLayout', JSON.stringify(v)); m.remove(); homePage(); };
  const m = modal(t('Customize home'), [
    h('label', { style: 'display:block;margin-bottom:12px' }, h('input', { type: 'checkbox', checked: banner, onchange: e => { banner = e.target.checked; } }), ' ', t('Banner at the top')),
    h('div', { class: 'dim', style: 'margin-bottom:6px' }, t('Rows')), box,
    h('div', { class: 'actions', style: 'margin-top:14px' },
      h('button', { class: 'primary', onclick: () => save({ order: list.map(r => r.id), hidden: list.filter(r => !r.on).map(r => r.id), banner }) }, t('Save')),
      h('button', { onclick: () => save({}) }, t('Reset')),
      h('button', { onclick: () => m.remove() }, t('Cancel')))]);
}
// Search results by kind (movies, shows, episodes …), the kinds in the order of their best hit.
const KIND_NAMES = { movie: 'Movies', show: 'Shows', season: 'Seasons', episode: 'Episodes', artist: 'Artists', album: 'Albums', track: 'Songs', clip: 'Videos', photo: 'Photos', collection: 'Collections' };
async function searchPage(q) {
  markNav('');
  const head = h('h1', {}, t('Search: {q}', { q }));
  const wait = skeletonSoon(() => set(head, skeletonGrid(12)));
  const r = meta(await GET('/hubs/search?query=' + encodeURIComponent(q)));
  wait();
  if (!r.length) return set(head, h('div', { class: 'empty' }, t('Nothing found.')));
  const groups = new Map();
  for (const it of r) { const k = KIND_NAMES[it.type] ? it.type : 'other'; if (!groups.has(k)) groups.set(k, []); groups.get(k).push(it); }
  if (groups.size === 1) return set(head, h('div', { class: 'grid' }, r.map(i => posterCard(i))));
  set(head, [...groups].map(([k, list]) => [
    h('h2', {}, t(KIND_NAMES[k] || 'Other'), h('span', { class: 'dim', style: 'font-weight:400;margin-left:8px;font-size:14px' }, list.length)),
    h('div', { class: 'grid', style: 'margin-bottom:26px' }, list.map(i => posterCard(i)))]));
}

const SORTS = [['title', t('Title')], ['year:desc', t('Year (newest)')], ['year', t('Year (oldest)')], ['added_at:desc', t('Recently added')], ['rating:desc', t('Rating')], ['duration:desc', t('Length')], ['last_viewed_at:desc', t('Recently watched')], ['view_count:desc', t('Most watched')], ['random', t('Random')]];

// Short names for the library filters (the list itself comes from the server).
const FILTER_NAMES = { genre: 'Genre', year: 'Year', decade: 'Decade', content_rating: 'Age rating', collection: 'Collection', director: 'Director', actor: 'Actor', writer: 'Writer', producer: 'Producer',
  country: 'Country', studio: 'Studio', network: 'Network', video_resolution: 'Resolution', hdr: 'HDR', dovi: 'Dolby Vision', hdr10plus: 'HDR10+', atmos: 'Dolby Atmos', unwatched: 'Unwatched', in_progress: 'In progress', unwatched_leaves: 'Unwatched', unmatched: 'Unmatched',
  video_codec: 'Video codec', audio_codec: 'Audio codec', subtitle_codec: 'Subtitle format', audio_layout: 'Audio channels', audio_language: 'Audio language',
  subtitle_language: 'Subtitle language', edition: 'Edition', label: 'Label', folder: 'Folder', duplicate: 'Duplicates' };
const RES_NAMES = { '4k': '4K', 1080: '1080p', 720: '720p', 480: '480p', sd: 'SD' };
// A button that opens a menu below it, as in the library bar.
// entries: a list or a function giving one (may be async); opts: {on, title, icon}.
function dropBtn(label, entries, opts = {}) {
  const b = h('button', { class: 'dd' + (opts.on ? ' on' : ''), 'aria-haspopup': 'menu', title: opts.title || null,
    onclick: async () => {
      if (b.classList.contains('open')) return closeMenus();
      const list = typeof entries === 'function' ? await entries() : entries;
      if (!list || !list.length) return;
      const m = openMenu(b, list, null, 'left');
      b.classList.add('open');
      new MutationObserver((_, o) => { if (!m.isConnected) { b.classList.remove('open'); o.disconnect(); } }).observe(document.body, { childList: true });
    } },
    opts.icon ? icon(opts.icon) : null, h('span', {}, label), icon('down'));
  return b;
}

// ---------- details
// Kinds of extras (trailers, featurettes …) as the scanner names them.
const ONLINE_VIDEO_TYPES = { Trailer: 'Trailer', Teaser: 'Teaser', Clip: 'Scene', Featurette: 'Featurette', 'Behind the Scenes': 'Behind the scenes' };
const EXTRA_TYPES = { trailer: 'Trailer', behindthescenes: 'Behind the scenes', deleted: 'Deleted scene', featurette: 'Featurette', interview: 'Interview', scene: 'Scene', short: 'Short', other: 'Extra' };

// A title's page while it loads: the poster's place and a few grey lines.
const skeletonItem = () => h('div', { class: 'detail' }, h('div', { class: 'poster skel', style: 'aspect-ratio:2/3' }),
  h('div', { class: 'info' }, h('div', { class: 'skel', style: 'height:30px;width:55%;margin:6px 0 16px' }), h('div', { class: 'skel', style: 'height:14px;width:30%;margin-bottom:24px' }),
    [90, 96, 70].map(w => h('div', { class: 'skel', style: `height:12px;width:${w}%;margin-bottom:10px` }))));
async function itemPage(id) {
  markNav('');
  // (Not when it is drawn again in place, after a change on it.)
  const wait = location.hash.startsWith('#/item/' + id) && main().querySelector('.detail[data-id="' + id + '"]') ? () => {} : skeletonSoon(() => set(skeletonItem()));
  const it = meta(await GET('/library/metadata/' + id))[0];
  wait();
  const m = it.Media && it.Media[0];
  // Everything else the page needs is asked for at once (not one after the other); the related rows come last, after the page is drawn.
  const topId = it.grandparentRatingKey || (it.type === 'season' ? it.parentRatingKey : null);
  const kidsP = ['show', 'season', 'artist', 'album'].includes(it.type) ? GET(`/library/metadata/${id}/children`) : null;
  const topP = topId ? GET('/library/metadata/' + topId) : null;
  const chaptersP = m ? GET(`/library/metadata/${id}/chapters`).catch(() => null) : null;
  const relatedP = (it.type === 'movie' || it.type === 'show') && (it.extra || {}).metadata_source ? GET(`/library/metadata/${id}/related?count=20`).catch(() => null) : null;
  // Merged by hand (admin: "Merge"): several files of the same title; the choice is remembered per title.
  const vers = (it.Media || []).length > 1 ? it.Media : null;
  let vi = vers ? Math.min(Number(store.get('version:' + id, 0)) || 0, vers.length - 1) : 0;
  const playV = (at) => play(vi > 0 ? Object.assign({}, it, { id: vers[vi].id, ratingKey: String(vers[vi].id), Media: undefined }) : it, at);
  const versionName = v => [v.height ? (v.height >= 2000 ? '4K' : v.height + 'p') : '', v.edition, v.size ? fmtSize(v.size) : ''].filter(Boolean).join(' · ');
  const kids = kidsP ? meta(await kidsP) : [];
  // A show: the episodes of the season its page opens on come with the page (no jump when they arrive).
  const seasonAt = it.type === 'show' && kids.length ? firstSeason(id, kids) : -1;
  const seasonEps = seasonAt >= 0 ? meta(await GET(`/library/metadata/${kids[seasonAt].id}/children`).catch(() => ({ MediaContainer: {} }))) : [];
  // Drawn again after a change, where it was scrolled to; not when another page is open by now (an "Undo" comes later).
  const refresh = async () => {
    if (!location.hash.startsWith('#/item/' + id)) return;
    const y = window.scrollY; await itemPage(id); window.scrollTo(0, y);
  };
  const watched = it.leafCount ? it.viewedLeafCount === it.leafCount : it.viewCount > 0;
  const crumbs = [];
  if (it.grandparentTitle) crumbs.push(h('a', { href: '#/item/' + it.grandparentRatingKey }, it.grandparentTitle), ' › ');
  if (it.parentTitle) crumbs.push(h('a', { href: '#/item/' + it.parentRatingKey }, seasonTitle(it.parentTitle)));
  const nextEp = () => playNextEpisode(id);
  // Online details (plot, cast, crew …) live in it.extra; seasons and episodes borrow the show's.
  const x = it.extra || {};
  const topX = topP ? ((meta(await topP)[0] || {}).extra || {}) : x;
  const cast = x.cast || (it.type !== 'movie' ? topX.cast : null) || [];
  const ids = x.ids || topX.ids || {};
  const crew = [[t('Directed by'), x.directors], [t('Written by'), x.writers], [t('Created by'), x.creators || (it.type !== 'show' ? topX.creators : null)]].filter(r => r[1] && r[1].length);
  const links = [ids.imdb ? ['IMDb', 'https://www.imdb.com/title/' + ids.imdb] : null, ids.tmdb ? ['TMDB', `https://www.themoviedb.org/${it.type === 'movie' ? 'movie' : 'tv'}/${ids.tmdb}`] : null,
    it.type === 'movie' && (ids.tmdb || ids.imdb) ? ['Letterboxd', ids.tmdb ? 'https://letterboxd.com/tmdb/' + ids.tmdb : 'https://letterboxd.com/imdb/' + ids.imdb] : null,
    ids.tvmaze ? ['TVmaze', 'https://www.tvmaze.com/shows/' + ids.tvmaze] : null,
    ids.anilist ? ['AniList', 'https://anilist.co/anime/' + ids.anilist] : null, ids.mal ? ['MyAnimeList', 'https://myanimelist.net/anime/' + ids.mal] : null, ids.wikipedia ? ['Wikipedia', ids.wikipedia] : null, ids.wikidata ? ['Wikidata', 'https://www.wikidata.org/wiki/' + ids.wikidata] : null].filter(Boolean);
  const matchable = it.type === 'movie' || it.type === 'show';
  const localTrailer = (it.Extras || []).find(e => e.extraType === 'trailer');
  const onlineVideos = matchable ? (x.trailers || []).filter(v => v && v.key) : [];
  // Rows: Other Editions, Related, More by the director, More with the first two actors.
  const chapters = chaptersP ? ((await chaptersP) || { MediaContainer: {} }).MediaContainer.Chapter || [] : [];
  const relatedBox = h('div', {});
  // The backdrop fades in once loaded; drawn again in place (after a change) it stays as it was.
  const heroSrc = withTok(`/library/metadata/${id}/art?v=${it.updatedAt || 0}`), oldHero = main().querySelector('img.hero.in');
  const hero = h('img', { class: 'hero' + (oldHero && oldHero.src === new URL(heroSrc, location.href).href ? ' in' : ''), src: heroSrc, alt: '',
    onload: e => e.target.classList.add('in'), onerror: e => e.target.remove() });
  set(
    hero,
    h('div', { class: 'dim', style: 'margin-bottom:10px;position:relative' }, crumbs),
    h('div', { class: 'detail', 'data-id': id },
      // A click anywhere on the poster plays.
      h('div', { class: 'poster' + (isPlayable(it) ? ' playable' : ''), 'data-id': id, onclick: () => { if (isPlayable(it)) playV((it.viewOffset || 0) / 1000); } }, it.thumb ? posterImg(it.thumb, '(max-width:760px) 180px, 240px') : h('img', { src: withTok(it.thumb), alt: '' }),
        isPlayable(it) ? h('button', { class: 'play', title: t('Play'), 'aria-label': t('Play'), onclick: e => { e.stopPropagation(); playV((it.viewOffset || 0) / 1000); } }, icon('play')) : null),
      h('div', { class: 'info' },
        h('h1', { style: 'margin-bottom:4px' }, it.type === 'episode' ? `${it.index}. ${it.title}` : it.type === 'season' ? seasonTitle(it.title) : it.clearLogo ? h('img', { src: withTok(it.clearLogo), alt: it.title, title: it.title, style: 'max-height:96px;max-width:100%;object-fit:contain;display:block', onerror: e => e.target.replaceWith(document.createTextNode(it.title)) }) : it.title,
          null),
        h('div', { class: 'dim' }, [it.year, it.editionTitle, it.type === 'episode' ? t('Season {n}', { n: it.parentIndex }) : null, fmtDur(it.duration), it.contentRating, (it.rating && !Object.keys(x.ratings || (it.type === 'season' ? topX.ratings : null) || {}).length) ? `★ ${fmtNum(it.rating)}` : null, it.leafCount ? t('{seen}/{all} watched', { seen: it.viewedLeafCount, all: it.leafCount }) : null].filter(Boolean).join(' · ')),
        scoreBadges(x.ratings || (it.type === 'season' ? topX.ratings : null)),
        it.tagline ? h('p', { style: 'font-style:italic' }, it.tagline) : null,
        it.summary ? h('p', { class: 'plot' }, it.summary) : (S.me.admin && matchable && !x.metadata_source ? h('p', { class: 'dim' }, x.unmatched_by_hand ? t('The match was removed — use "Match" to pick one, or "Refresh metadata" to look again.') : x.metadata_status === 'unmatched' ? t('No details were found online for this title — use "Match" to pick the right one.') : t('Details are being looked up online…')) : null),
        it.genres && it.genres.length ? h('div', {}, it.genres.map(g => h('span', { class: 'tag' }, g))) : null,
        crew.length || it.studio || x.originally_available || x.original_title ? h('div', { class: 'kv credits' },
          crew.map(([k, v]) => [h('div', { class: 'dim' }, k), h('div', {}, v.join(', '))]),
          it.studio ? [h('div', { class: 'dim' }, it.type === 'show' ? t('Network') : t('Studio')), h('div', {}, it.studio)] : null,
          x.original_title && x.original_title !== it.title ? [h('div', { class: 'dim' }, t('Original title')), h('div', {}, x.original_title)] : null,
          x.originally_available ? [h('div', { class: 'dim' }, it.type === 'movie' ? t('Released') : t('First aired')), h('div', {}, new Date(x.originally_available).toLocaleDateString(I18N.lang))] : null) : null,
        links.length ? h('div', { class: 'dim', style: 'margin-top:6px' }, t('More: '), links.map(([n, u], i) => [i ? ' · ' : '', h('a', { href: u, target: '_blank', rel: 'noopener' }, n)])) : null,
        // Play first; the toggles as round icons (their state shows in colour); everything else under "⋯".
        h('div', { class: 'actions item-actions' },
          m && it.missing ? h('button', { class: 'big unavailable', disabled: true, title: t('The file is gone from disk; the item is in the library’s trash') }, t('Unavailable')) : null,
          m && !it.missing && resumeAt(it) ? h('button', { class: 'primary big', onclick: () => playV(resumeAt(it)) }, t('▶ Resume from {time}', { time: fmtTime(resumeAt(it)) })) : null,
          m && !it.missing && resumeAt(it) ? iconBtn('restart', t('▶ Play from start'), () => playV(0)) : null,
          m && !it.missing && !resumeAt(it) ? h('button', { class: 'primary big', onclick: () => playV(0) }, t('▶ Play')) : null,
          vers ? h('select', { title: t('Version'), 'aria-label': t('Version'), onchange: e => { vi = Number(e.target.value); store.set('version:' + id, vi); } },
            vers.map((v, i) => h('option', { value: i, selected: i === vi }, versionName(v)))) : null,
          // A trailer file next to the movie first, else the first online trailer.
          localTrailer ? h('button', { class: 'big', onclick: () => play(localTrailer, 0) }, t('▶ Trailer'))
            : onlineVideos.length ? h('button', { class: 'big', onclick: () => playOnlineVideo(onlineVideos[0]) }, t('▶ Trailer')) : null,
          !m && it.leafCount && (it.type === 'artist' || it.type === 'album') ? [
            h('button', { class: 'primary big', onclick: () => playMusic(id, false) }, t('▶ Play')),
            h('button', { class: 'big', title: t('Random order; the same show or artist does not come twice in a row while others are left'), onclick: () => playMusic(id, true) }, t('Shuffle'))] : null,
          !m && it.leafCount && it.type !== 'artist' && it.type !== 'album' ? h('button', { class: 'primary big', onclick: nextEp }, t('▶ Play next episode')) : null,
          iconBtn('check', watched ? t('Mark as unwatched') : t('Mark as watched'), () => markWatched([it], !watched, refresh), watched),
          iconBtn(it.favorite ? 'heart' : 'heartOff', it.favorite ? t('Remove from favorites') : t('Add to favorites'), async () => { await api('PUT', `/api/items/${it.id}/favorite`, { favorite: !it.favorite }); refresh(); }, !!it.favorite),
          it.type === 'clip' ? null : iconBtn(it.watchlist ? 'mark' : 'markOff', it.watchlist ? t('Remove from watchlist') : t('Add to watchlist'), async () => { await toggleWatchlist(it); refresh(); }, !!it.watchlist),
          S.me.admin && it.type !== 'clip' ? iconBtn('edit', t('Edit details'), async () => editItem(await fullItem(it), refresh)) : null,
          iconBtn('more', t('More'), e => itemMenu(it, e.currentTarget, refresh, { detail: true })),
          starRating(it)),
        m ? h('div', { class: 'panel kv' },
          h('div', {}, t('File type')), h('div', {}, `${friendlyContainer(m)} · ${fmtSize(m.size)}`),
          it.type === 'track' ? null : [h('div', {}, t('Video')), h('div', {}, m.videoCodec ? [String(m.videoCodec).toUpperCase(), `${m.width}×${m.height}`, m.bitDepth > 8 ? t('{n}-bit', { n: m.bitDepth }) : ''].filter(Boolean).join(' · ') : t('unknown (ffprobe not available)'))],
          h('div', {}, t('Audio')), h('div', {}, audioTracks(m)),
          h('div', {}, t('Bitrate')), h('div', {}, m.bitrate ? fmtNum(m.bitrate / 1000, 1) + ' Mbit/s' : '–'),
          it.type === 'track' ? null : [h('div', {}, t('Subtitles')), h('div', {}, [...new Set((m.subtitles || []).map(s => s.language ? langName(s.language) : s.file || s.codec))].join(', ') || t('none'))],
          m.file ? [h('div', {}, t('Path')), h('div', { style: 'word-break:break-all' }, m.file)] : null,
          (m.Part || []).length > 1 ? [h('div', {}, t('Parts')), h('div', {}, t('{n} files, played one after the other', { n: m.Part.length }))] : null,
          h('div', {}, t('In your browser')), h('div', { id: 'decision', class: 'dim' }, '…')) : null)),
    // A show's or season's episodes come first, what one came for; the cast after them.
    seasonAt >= 0 ? seasonTabs(id, kids, seasonAt, seasonEps, refresh) : null,
    kids.length && it.type === 'season' ? [h('h2', {}, t('Episodes')), episodeCards(kids, refresh)] : null,
    cast.length ? [h('h2', {}, t('Cast')), scroller('row cast', cast.map(p => {
      const initials = () => h('div', { class: 'initials' }, (p.name || '?').split(' ').map(w => w[0]).slice(0, 2).join(''));
      // The photo saved on this server; the original web address only as a fallback.
      const src = p.thumb ? withTok(p.thumb) : p.photo;
      const pic = src ? h('img', { src, alt: '', loading: 'lazy', referrerpolicy: 'no-referrer', onerror: e => e.target.replaceWith(initials()) }) : initials();
      // Actors are shared: the name opens a page with everything they are in.
      return h('a', { class: 'person', href: `#/person/${encodeURIComponent(p.key || 'byname')}?name=${encodeURIComponent(p.name || '')}` }, pic, h('b', {}, p.name), p.role ? h('div', { class: 'dim' }, p.role) : null,
        p.count > 1 ? h('div', { class: 'dim' }, t('{n} in this library', { n: p.count })) : null);
    }))] : null,
    it.type === 'artist' && kids.length ? RELEASES.map(r => {
      const of = kids.filter(k => (k.releaseType || 'album') === r);
      return of.length ? [h('h2', {}, releaseName(r, true)), scroller('row', of.map(k => posterCard(k)))] : null;
    }) : null,
    kids.length && it.type !== 'artist' && it.type !== 'show' && it.type !== 'season' ? [h('h2', {}, it.type === 'album' ? t('Songs') : t('Episodes')),
        h('div', { class: 'list' }, kids.map((e, i) => h('div', { class: 'li' + (e.missing ? ' missing' : '') },
          it.type === 'album' ? null : h('img', { src: withTok(e.thumb), loading: 'lazy' }),
          h('div', { class: 'grow' }, h('a', { href: '#/item/' + e.id }, h('b', {}, e.index ? `${e.index}. ${e.title}` : e.title)), h('div', { class: 'dim' }, [e.missing ? t('Unavailable') : '', fmtDur(e.duration), e.viewCount ? t('✓ watched') : '', resumeAt(e) ? t('stopped at {time}', { time: fmtTime(resumeAt(e)) }) : ''].filter(Boolean).join(' · ')),
            e.summary ? h('div', { class: 'ep-plot' }, e.summary) : null),
          e.missing ? h('span', { class: 'gone-ico', title: t('The file is gone from disk; the item is in the library’s trash') }, icon('trash'))
            : h('button', { class: 'primary small', onclick: () => play(e, e.viewOffset ? e.viewOffset / 1000 : 0, kids.slice(i + 1)) }, '▶'))))] : null,
    chapters.length ? [h('h2', {}, t('Chapters')), scroller('row extras', chapters.map(c => h('div', { class: 'card', onclick: () => play(it, c.start / 1000) },
      h('div', { class: 'poster' }, h('img', { src: withTok(c.thumb), loading: 'lazy', alt: '' })),
      h('div', { class: 't', title: c.title }, c.title.replace(/^Chapter (\d+)$/, (_, n) => t('Chapter {n}', { n }))), h('div', { class: 's' }, fmtTime(c.start / 1000)))))] : null,
    (it.Extras || []).length || onlineVideos.length ? [h('h2', {}, t('Extras')), scroller('row extras', [...(it.Extras || []).map(x => h('div', { class: 'card', onclick: () => play(x, 0) },
      h('div', { class: 'poster' }, h('img', { src: withTok(x.thumb), loading: 'lazy', alt: '' })),
      h('div', { class: 't', title: t(x.title) }, t(x.title)), h('div', { class: 's' }, [t(EXTRA_TYPES[x.extraType] || 'Extra'), fmtDur(x.duration)].filter(Boolean).join(' · ')))),
      // Online videos (TMDB / Wikidata, played from YouTube), after the files.
      ...onlineVideos.map(v => h('div', { class: 'card', onclick: () => playOnlineVideo(v) },
        h('div', { class: 'poster' }, h('img', { src: `https://i.ytimg.com/vi/${encodeURIComponent(v.key)}/mqdefault.jpg`, loading: 'lazy', alt: '', referrerpolicy: 'no-referrer' })),
        h('div', { class: 't', title: v.name }, v.name), h('div', { class: 's' }, [t(ONLINE_VIDEO_TYPES[v.type] || 'Extra'), t('online')].join(' · '))))])] : null,
    relatedBox);
  if (relatedP) relatedP.then(r => relatedBox.replaceChildren(...((r && r.MediaContainer.Hub) || []).flatMap(hb => row(hb.title, hb.Metadata || []) || [])));
  if (m) {
    try {
      const d = await GET(`/library/metadata/${id}/decision`);
      const words = { direct: t('Plays the original file directly'), remux: t('Repackaged on the fly (picture untouched)'), transcode: t('Converted on the fly') };
      $('#decision').textContent = words[d.method] + (d.reasons.length ? ' — ' + d.reasons.map(r => t(r)).join('; ') : '');
    } catch (e) { /* ignore */ }
  }
}

// Your own rating: five stars, each worth 2 on the server's 0-10 scale.
function starRating(it) {
  let cur = it.userRating || 0;
  const box = h('span', { class: 'stars', title: t('Your rating') });
  const clear = h('button', { class: 'clear', title: t('Remove your rating') }, t('clear'));
  // Half stars: each point of the 0–10 scale is half a star; the left half of a
  // star gives the half, the right half the whole star.
  const paint = (v = cur) => {
    const pts = Math.round(v);
    [...box.children].forEach((b, i) => { b.classList.toggle('on', (i + 1) * 2 <= pts); b.classList.toggle('half', (i + 1) * 2 - 1 === pts); });
    clear.style.visibility = cur ? '' : 'hidden';
  };
  const stars = v => (v / 2).toLocaleString(I18N.lang, { maximumFractionDigits: 1 });
  const pointsAt = (e, i) => { const r = e.currentTarget.getBoundingClientRect(); return i * 2 - (e.clientX - r.left < r.width / 2 ? 1 : 0); };
  const save = async v => {
    await api('POST', `/:/rate?key=${it.id}&rating=${v}`);
    cur = v < 0 ? 0 : v; it.userRating = cur; paint();
    toast(v < 0 ? t('Rating removed') : t('Rated {n} of 5 stars', { n: stars(v) }));
  };
  for (let i = 1; i <= 5; i++) box.append(h('button', { 'aria-label': t('{n} stars (click the left half for {half})', { n: i, half: stars(i * 2 - 1) }), title: t('{half} or {n} of 5', { half: stars(i * 2 - 1), n: i }),
    onmousemove: e => paint(pointsAt(e, i)), onclick: e => save(e.detail === 0 ? i * 2 : pointsAt(e, i)) }, '★'));
  box.onmouseleave = () => paint();
  clear.onclick = () => save(-1);
  paint();
  return h('span', { style: 'display:inline-flex;align-items:center' }, box, clear);
}

// Marks for the review sites, drawn here (no pictures are fetched). Rotten
// Tomatoes shows its tomato for 60 % and more and the green splat below, and
// Metacritic's square takes the site's colour for the score.
const SCORE_MARKS = {
  imdb: () => '<svg viewBox="0 0 64 32" aria-hidden="true"><rect width="64" height="32" rx="5" fill="#f5c518"/><text x="32" y="23" text-anchor="middle" font-family="Arial,Helvetica,sans-serif" font-weight="900" font-size="19" fill="#000">IMDb</text></svg>',
  rotten_tomatoes: pct => pct >= 60
    ? '<svg viewBox="0 0 32 32" aria-hidden="true"><circle cx="16" cy="18.5" r="12" fill="#fa320a"/><path fill="#00912d" d="M10 5.5c2.6.4 4.6 1.8 5.8 4 .9-2.3 2.8-3.6 5.4-3.7-.8 1.6-1.9 2.7-3.3 3.5 2 .1 3.8.8 5.1 2.2-2.4.5-4.8.3-7-.6-1.9 1.3-4.2 1.8-6.8 1.4 1.4-1.2 2.9-2 4.6-2.3-1.6-1-2.9-2.5-3.8-4.5z"/></svg>'
    : '<svg viewBox="0 0 32 32" aria-hidden="true"><path fill="#0ac855" d="M16 2.5l2.6 6.4 5.9-3.3-1.6 6.7 6.6 1.6-5.6 4 3.8 5.8-6.8-.9-.5 6.9-4.4-5.2-4.4 5.2-.5-6.9-6.8.9 3.8-5.8-5.6-4 6.6-1.6-1.6-6.7 5.9 3.3z"/></svg>',
  metacritic: n => `<svg viewBox="0 0 32 32" aria-hidden="true"><rect width="32" height="32" rx="6" fill="${n >= 61 ? '#00ce7a' : n >= 40 ? '#ffbd3f' : '#ff6874'}"/><text x="16" y="23" text-anchor="middle" font-family="Arial,Helvetica,sans-serif" font-weight="900" font-size="19" fill="#1a1a1a">M</text></svg>`,
  // The audience score of Rotten Tomatoes: a bucket of popcorn, red below 60 %.
  rotten_tomatoes_audience: pct => `<svg viewBox="0 0 32 32" aria-hidden="true"><path fill="${pct >= 60 ? '#fa320a' : '#7a7f86'}" d="M8 14h16l-2 15H10z"/><path fill="#fff" d="M12 14h2l.5 15h-2zm6 0h2l-.5 15h-2z" opacity=".55"/><g fill="#ffd34d"><circle cx="10" cy="11" r="3.4"/><circle cx="16" cy="8.5" r="4"/><circle cx="22" cy="11" r="3.4"/><circle cx="13" cy="12.5" r="3"/><circle cx="19" cy="12.5" r="3"/></g></svg>`,
  // AniList's average score, in percent.
  anilist: () => '<svg viewBox="0 0 32 32" aria-hidden="true"><rect width="32" height="32" rx="6" fill="#152232"/><text x="16" y="22" text-anchor="middle" font-family="Arial,Helvetica,sans-serif" font-weight="900" font-size="15" fill="#3db4f2">AL</text></svg>',
  tmdb: () => '<svg viewBox="0 0 70 32" aria-hidden="true"><defs><linearGradient id="tmdb-mark" x1="0" x2="1"><stop offset="0" stop-color="#90cea1"/><stop offset=".56" stop-color="#3cbec9"/><stop offset="1" stop-color="#00b3e5"/></linearGradient></defs><rect width="70" height="32" rx="16" fill="url(#tmdb-mark)"/><text x="35" y="22" text-anchor="middle" font-family="Arial,Helvetica,sans-serif" font-weight="900" font-size="16" fill="#0d253f">TMDB</text></svg>',
};

// Scores from review sites with their marks. Values are as the source gives them.
function scoreBadges(r) {
  if (!r) return null;
  const names = [['imdb', 'IMDb'], ['rotten_tomatoes', 'Rotten Tomatoes'], ['rotten_tomatoes_audience', 'Rotten Tomatoes audience'], ['metacritic', 'Metacritic'], ['tmdb', 'TMDB'], ['anilist', 'AniList']];
  const shown = names.filter(([k]) => r[k] && r[k].score);
  if (!shown.length) return null;
  return h('div', { class: 'scores' }, shown.map(([k, n]) => {
    const s = r[k];
    const count = s.count ?? s.votes;
    const many = count ? t(s.count_label === 'reviews' ? '{n} reviews' : '{n} votes', { n: Number(count).toLocaleString(I18N.lang) }) : '';
    const mark = h('span', { class: 'mark' });
    const num = parseFloat(String(s.score)) || 0;
    mark.innerHTML = SCORE_MARKS[k](k === 'rotten_tomatoes_audience' ? (String(s.score).includes('/5') ? num * 20 : String(s.score).includes('/10') ? num * 10 : num) : num);
    const kids = [mark, h('span', { class: 'num' }, h('b', {}, s.score), h('span', {}, many || t(n)))];
    const title = `${t(n)}: ${s.score}${many ? ' · ' + many : ''}${s.note ? ' — ' + s.note : ''}`;
    return s.url ? h('a', { class: 'score', href: s.url, target: '_blank', rel: 'noopener', title, 'aria-label': title }, kids)
      : h('div', { class: 'score', title, 'aria-label': title }, kids);
  }));
}

// ---------- background work (details downloads, credits detection) in the top bar
const ACT = { done: -1, timer: null, lastRefresh: 0, open: false };
// A click on the bar opens what is running, what waits and what is planned after it.
async function drawActPanel() {
  let box = $('#actpanel');
  if (!ACT.open) { if (box) box.remove(); return; }
  let d;
  try { d = await GET('/api/activity'); } catch (e) { return; }
  if (!ACT.open) return;
  if (!box) { box = h('div', { id: 'actpanel', class: 'actpanel' }); document.body.append(box); }
  const sec = (title, ...kids) => h('div', { class: 'actsec' }, h('h4', {}, title), ...kids);
  const list = (items, more) => h('ul', {}, items.map(x => h('li', {}, x)), more > 0 ? h('li', { class: 'dim' }, t('… and {n} more', { n: more })) : null);
  const blocks = [];
  if (d.job) {
    const j = d.job, pct = Math.floor(100 * j.done / Math.max(1, j.total));
    const head = j.label === 'Optimizing' ? t('{label}: {pct} %', { label: t(j.label), pct }) : t('{label}: {done} of {total}', { label: t(j.label), done: j.step, total: j.steps });
    blocks.push(sec(t('Running now'), h('div', { style: 'display:flex;gap:10px;align-items:center' }, h('span', {}, head), h('span', { class: 'pbar' }, h('i', { style: `width:${pct}%` }))),
      j.items.length ? [h('div', { class: 'dim', style: 'margin-top:6px' }, t('Next up')), list(j.items, j.more)] : null));
  }
  if (d.scanning.length) blocks.push(sec(t('Scanning'), list(d.scanning, 0)));
  const dt = d.details;
  if (dt.working.length || dt.total) {
    blocks.push(sec(t('Downloading details'), dt.working.length ? h('div', {}, '\u25B6 ' + dt.working.join(', ')) : null,
      dt.total ? [h('div', { class: 'dim', style: 'margin-top:6px' }, t('Waiting ({n})', { n: dt.total })), list(dt.queued, dt.more)] : null));
  }
  const modes = { asap: t('as soon as possible'), scheduled: t('once a day') };
  const plan = [];
  const add = (n, text, mode, ex) => { if (n) plan.push(h('div', {}, text + (modes[mode] ? ' \u00B7 ' + modes[mode] : '')), ex.length ? list(ex, n - ex.length) : null); };
  add(d.next.credits.n, t('Credits detection: {n} videos waiting', { n: d.next.credits.n }), d.next.credits.mode, d.next.credits.examples);
  add(d.next.intros.n, t('Intro detection: {n} seasons waiting', { n: d.next.intros.n }), d.next.intros.mode, d.next.intros.examples);
  blocks.push(sec(t('Planned afterwards'), plan.length ? plan : h('div', { class: 'dim' }, t('Nothing else is planned.'))));
  if (d.last && d.last.message) blocks.push(sec(t('Last result'), h('div', {}, (d.last.ok === false ? '\u26A0 ' : '\u2713 ') + t(d.last.message))));
  box.replaceChildren(h('button', { class: 'small close', title: t('Close'), onclick: () => { ACT.open = false; box.remove(); } }, '\u2715'), ...blocks);
}
document.addEventListener('click', e => {
  const bar = $('#activity');
  if (bar.contains(e.target) && !e.target.closest('button')) { ACT.open = !ACT.open; drawActPanel(); }
  else if (ACT.open && !e.target.closest('#actpanel')) { ACT.open = false; drawActPanel(); }
});
async function pollActivity() {
  clearTimeout(ACT.timer);
  const el = $('#activity');
  if (!S.me || !S.me.admin) { el.style.display = 'none'; return; }
  let st;
  try { st = await GET('/api/metadata/status'); } catch (e) { ACT.timer = setTimeout(pollActivity, 10000); return; }
  drawPlayingBtn(st.playing);
  const scanning = st.scanning || [];
  const busy = st.queued > 0 || st.working_on || st.job || scanning.length;
  // The poster being updated right now shows a spinner (library and detail page).
  document.querySelectorAll('.poster.busy').forEach(e => e.classList.remove('busy'));
  for (const wid of (st.working_ids || (st.working_on ? [st.working_on] : []))) document.querySelectorAll(`.poster[data-id="${wid}"]`).forEach(e => e.classList.add('busy'));
  const parts = [];
  // A scan says where it is: listing folders, then file n of m (with the file in the tooltip), then finishing.
  const scanNote = p => p.phase === 'reading' && p.total ? t('Scanning {name}: {done} of {total}', { name: p.name, done: fmtNum(p.done), total: fmtNum(p.total) })
    : p.phase === 'finishing' ? t('Scanning {name}: finishing', { name: p.name })
    : p.done ? t('Scanning {name}: looking through the folders, {n} videos found', { name: p.name, n: fmtNum(p.done) })
    : t('Scanning {name}: looking through the folders', { name: p.name });
  const scans = st.scan_progress || [];
  for (const name of scanning.filter(n => !scans.some(p => p.name === n))) parts.push(h('span', { class: 'txt' }, t('Scanning {name}', { name })));
  for (const p of scans) {
    parts.push(h('span', { class: 'txt' }, scanNote(p)));
    if (p.phase === 'reading' && p.total) parts.push(h('span', { class: 'pbar' }, h('i', { style: `width:${Math.round(100 * p.done / p.total)}%` })));
  }
  // Optimizing counts in hundredths of a video: shown as a percentage.
  if (st.job) parts.push(h('span', { class: 'txt' }, st.job.label === 'Optimizing' ? t('{label}: {pct} %', { label: t(st.job.label), pct: Math.floor(100 * st.job.done / Math.max(1, st.job.total)) })
    : t('{label}: {done} of {total}', { label: t(st.job.label), done: st.job.done, total: st.job.total })), h('span', { class: 'pbar' }, h('i', { style: `width:${Math.round(100 * st.job.done / Math.max(1, st.job.total))}%` })));
  if (st.working_on || st.queued > 0) parts.push(h('span', { class: 'txt' }, t('Updating details') + (st.working_title ? ': ' + st.working_title : '') + (st.queued > 0 ? ' · ' + t('{n} left', { n: st.queued }) : '')));
  if (busy) {
    // Everything that goes file by file can be stopped (after the file it is on); waiting details are dropped.
    if (st.job || st.queued > 0) {
      parts.push(h('button', { class: 'small', onclick: async e => {
        e.target.disabled = true;
        try { await api('POST', '/api/job/cancel'); toast(t('Stopping after the current file…')); } catch (er) { toast(er.message, 6000); }
        pollActivity();
      } }, t('Cancel')));
    }
    el.replaceChildren(h('span', { class: 'spin' }), ...parts);
    el.title = [...scans.map(p => scanNote(p) + (p.current ? '\n' + p.current : '')), ...scanning.filter(n => !scans.some(p => p.name === n)).map(n => t('Scanning {name}', { name: n })), st.job ? t('{label}: {done} of {total}', { label: t(st.job.label), done: st.job.done, total: st.job.total }) : '',
      st.working_on || st.queued > 0 ? t('Updating details') + (st.working_title ? ': ' + st.working_title : '') + (st.queued > 0 ? ' · ' + t('{n} left', { n: st.queued }) : '') : '',
      st.last && st.last.message ? t('Last: {msg}', { msg: t(st.last.message) }) : ''].filter(Boolean).join('\n') || t('Background work on the server');
    el.style.display = 'flex';
  } else if (ACT.done >= 0 && st.done > ACT.done && st.last && st.last.message) {
    el.replaceChildren(h('span', { class: 'txt' }, (st.last.ok === false ? '⚠ ' : '✓ ') + t(st.last.message)));
    el.style.display = 'flex';
    setTimeout(() => { if (!$('#activity .spin')) el.style.display = 'none'; }, 8000);
  }
  // Show new details as soon as they arrive (unless something is open on top).
  // A picked selection in the library stays: the page is not redrawn under it.
  if (ACT.done >= 0 && st.done > ACT.done && !document.querySelector('.modal') && !document.querySelector('.grid.picking') && $('#player').style.display !== 'flex') {
    const hash = location.hash.slice(1) || '/';
    const onItem = hash.match(/^\/item\/(\d+)/);
    const fresh = Date.now() - ACT.lastRefresh > 8000;
    if (onItem ? (!st.last || !st.last.id || String(st.last.id) === onItem[1] || !busy) : (/^\/(section\/|$)/.test(hash) && fresh && scrollY < 200)) {
      ACT.lastRefresh = Date.now();
      const y = scrollY; await render(); scrollTo(0, y);
    }
  }
  ACT.done = st.done;
  if (ACT.open) drawActPanel();
  ACT.timer = setTimeout(pollActivity, busy ? 2500 : 8000);
}

// The owner's "playing now" button in the top bar: grey, or yellow with the count.
function drawPlayingBtn(n) {
  const b = $('#npbtn');
  if (!b || n === null || n === undefined) return;
  b.classList.toggle('live', n > 0);
  b.lastChild.textContent = n > 0 ? String(n) : '';
  const label = n > 0 ? t('Dashboard · {n} playing now', { n }) : t('Dashboard · nothing is playing');
  b.title = label; b.setAttribute('aria-label', label);
}

// onclose runs however the box goes away: its buttons, a click beside it or Escape.
// An online trailer (YouTube) in a window; nothing is stored on the server.
function playOnlineVideo(v) {
  modal(v.name || t('Trailer'), [h('div', { class: 'yt' }, h('iframe', {
    src: `https://www.youtube-nocookie.com/embed/${encodeURIComponent(v.key)}?autoplay=1&rel=0`,
    allow: 'autoplay; encrypted-media; picture-in-picture; fullscreen', allowfullscreen: true, referrerpolicy: 'strict-origin-when-cross-origin', title: v.name || 'Trailer' })),
    h('div', { class: 'dim', style: 'margin-top:8px;font-size:12px' }, t('Online video from {site}', { site: v.site || 'YouTube' }))]);
}

function modal(title, kids, onclose) {
  // Closes only when the click both started and ended on the backdrop (selecting text and letting go outside the box must not).
  let down = null;
  const box = h('div', { class: 'modal', onmousedown: e => { down = e.target; }, onclick: e => { if (e.target === box && down === box) box.remove(); down = null; } }, h('div', { class: 'box' }, h('h2', { style: 'margin-top:0' }, title), kids));
  const remove = box.remove.bind(box);
  box.remove = () => { if (!box.isConnected) return; remove(); if (onclose) onclose(); };
  document.body.append(box);
  return box;
}
document.addEventListener('keydown', e => {
  const boxes = document.querySelectorAll('.modal');
  if (e.key === 'Escape' && boxes.length) { e.stopImmediatePropagation(); boxes[boxes.length - 1].remove(); }
}, true);

// In-app stand-ins for the browser's prompt() and confirm().
// ask() gives the text, or null when cancelled; sure() gives true or false.
function ask(title, { value = '', placeholder = '', password = false, note = null, ok = t('OK') } = {}) {
  return new Promise(res => {
    let out = null;
    const inp = h('input', { type: password ? 'password' : 'text', value, placeholder, autocomplete: password ? 'new-password' : 'off', style: 'width:100%',
      onkeydown: e => { if (e.key === 'Enter') { out = inp.value; box.remove(); } } });
    const box = modal(title, [note ? h('p', { class: 'dim' }, note) : null, inp,
      h('div', { class: 'actions' }, h('button', { class: 'primary', onclick: () => { out = inp.value; box.remove(); } }, ok), h('button', { onclick: () => box.remove() }, t('Cancel')))],
      () => res(out));
    inp.focus(); inp.select();
  });
}
function sure(text, { ok = t('OK'), danger = false } = {}) {
  return new Promise(res => {
    let out = false;
    const yes = h('button', { class: danger ? 'danger' : 'primary', onclick: () => { out = true; box.remove(); } }, ok);
    const box = modal(text, [h('div', { class: 'actions' }, yes, h('button', { onclick: () => box.remove() }, t('Cancel')))], () => res(out));
    yes.focus();
  });
}

// Like GitHub's: for deletions that hurt, the name has to be typed before the button works.
function sureTyped(text, word, { ok = t('Delete') } = {}) {
  return new Promise(res => {
    let out = false;
    const yes = h('button', { class: 'danger', disabled: true, onclick: () => { out = true; box.remove(); } }, ok);
    const inp = h('input', { placeholder: word, autocomplete: 'off', style: 'width:100%', oninput: () => { yes.disabled = inp.value.trim() !== word; },
      onkeydown: e => { if (e.key === 'Enter' && !yes.disabled) yes.click(); } });
    const box = modal(text, [h('p', { class: 'dim' }, t('Type “{name}” to confirm.', { name: word })), inp,
      h('div', { class: 'actions' }, yes, h('button', { onclick: () => box.remove() }, t('Cancel')))], () => res(out));
    inp.focus();
  });
}

// Pick the right online entry when the automatic match was wrong or missing.
function fixMatch(it, done) {
  const x = it.extra || {};
  const onEnter = e => { if (e.key === 'Enter') { e.preventDefault(); search(); } };
  const title = h('input', { value: x.file_title || it.title, style: 'width:100%', onkeydown: onEnter });
  const year = h('input', { value: it.year || '', style: 'width:100px', onkeydown: onEnter });
  const results = h('div', { class: 'list', style: 'max-height:55vh;overflow:auto;margin-top:10px' });
  const search = async () => {
    results.replaceChildren(h('div', { class: 'dim' }, t('Searching…')));
    try {
      const r = await GET(`/library/metadata/${it.id}/matches?title=${encodeURIComponent(title.value)}&year=${encodeURIComponent(year.value)}`);
      if (!r.matches.length) return results.replaceChildren(h('div', { class: 'dim' }, t('Nothing found. Try another spelling or leave the year empty.')));
      results.replaceChildren(...r.matches.map(c => h('div', { class: 'li', style: 'cursor:pointer', onclick: async () => {
        // The dialog closes at once; the details come in the background and the page refreshes when they are there.
        box.remove(); toast(t('Downloading details for {title}…', { title: c.title }), 60000);
        try { const a = await api('PUT', `/library/metadata/${it.id}/match`, { source: c.source, id: c.id }); toast(t(a.message), 4000); if (done) done(); else if (location.hash === '#/item/' + it.id) itemPage(it.id); }
        catch (e) { toast(e.message, 8000); }
      } },
        c.thumb ? h('img', { src: c.thumb, referrerpolicy: 'no-referrer' }) : h('div', { class: 'initials', style: 'width:46px;height:68px' }, c.title[0] || '?'),
        h('div', { class: 'grow' }, h('b', {}, c.title), c.year ? ` (${c.year})` : '', h('div', { class: 'dim' }, `${c.source} · ${t('fit {n}%', { n: Math.round(Math.max(0, Math.min(1, c.score)) * 100) })}`),
          c.summary ? h('div', { class: 'ep-plot' }, c.summary) : null))));
    } catch (e) { results.replaceChildren(h('div', { class: 'err' }, e.message)); }
  };
  const box = modal(x.metadata_source ? t('Fix match') : t('Match'), [
    formRow(t('Title'), title), formRow(t('Year'), year),
    h('div', { class: 'actions' }, h('button', { class: 'primary', onclick: search }, t('Search')), h('button', { onclick: () => box.remove() }, t('Close'))),
    results]);
  search();
  title.focus(); title.select();
}

// One picture of an item (poster, background, logo, square poster): shown, replaced from a file or an address, or removed.
function imagePicker(it, slot, url, has) {
  const show = h('div', { style: 'margin:6px 0 12px' });
  const draw = on => show.replaceChildren(on && url
    ? h('img', { src: withTok(url + (url.includes('?') ? '&' : '?') + 't=' + Date.now()), alt: '', style: 'max-width:100%;max-height:260px;border-radius:6px;background:var(--panel)', onerror: e => e.target.replaceWith(h('div', { class: 'dim' }, t('No picture'))) })
    : h('div', { class: 'dim' }, t('No picture')));
  const send = async body => { try { await api('PUT', `/library/metadata/${it.id}/image/${slot}`, body); url = url || `/library/metadata/${it.id}/${slot === 'poster' ? 'thumb' : slot}`; draw(true); toast(t('Picture saved')); } catch (e) { toast(e.message, 4000); } };
  const file = h('input', { type: 'file', accept: 'image/jpeg,image/png,image/webp,image/gif', style: 'display:none', onchange: () => {
    const f = file.files[0]; if (!f) return;
    if (f.size > 3 * 1024 * 1024) return toast(t('The picture is too big (3 MB at most)'), 4000);
    const r = new FileReader(); r.onload = () => send({ data: r.result }); r.readAsDataURL(f);
  } });
  const addr = h('input', { placeholder: 'https://…', style: 'width:100%' });
  draw(has);
  return h('div', {}, show,
    h('div', { class: 'actions', style: 'justify-content:flex-start' }, h('button', { onclick: () => file.click() }, t('Upload a picture')), file,
      h('button', { class: 'danger', onclick: async () => { try { await api('DELETE', `/library/metadata/${it.id}/image/${slot}`); draw(false); toast(t('Picture removed')); } catch (e) { toast(e.message, 4000); } } }, t('Remove'))),
    formRow(t('From an address'), addr, null),
    h('button', { onclick: () => addr.value.trim() && send({ url: addr.value.trim() }) }, t('Use this address')));
}

// saved() runs after saving; without it the item's page opens.
// Tabs: General, Tags, Labels, Info. Lists are typed comma separated.
async function editItem(it, saved) {
  const f = {}, x = it.extra || {};
  const langs = ((await GET('/api/settings').catch(() => null)) || {}).languages || [];
  const list = v => (v || []).join(', ');
  const field = (k, label, val, o = {}) => formRow(label, (f[k] = h(o.area ? 'textarea' : 'input', { value: val ?? '', type: o.type || 'text', style: 'width:100%' })), o.help, o.area);
  const movie = it.type === 'movie' || it.type === 'episode';
  const fmtAt = ts => ts ? new Date(ts * (ts < 1e12 ? 1000 : 1)).toLocaleString(I18N.lang) : '';
  // Intro and credits markers (movies and episodes): shown as h:mm:ss, saved with the rest.
  const mk = movie ? await GET(`/library/metadata/${it.id}/markers`).catch(() => null) : null;
  const clock = ms => (ms || ms === 0) ? fmtTime(ms / 1000) : '';
  const unclock = v => { const p = v.trim().split(':').map(Number); if (!v.trim() || p.some(n => isNaN(n) || n < 0)) return null; return p.reduce((a, b) => a * 60 + b, 0) * 1000; };
  const mf = {}, mInit = mk ? { cs: clock((mk.credits || [])[0]), is: clock((mk.intro || [])[0]), ie: clock((mk.intro || [])[1]) } : {};
  const srcText = (src, errors) => ({ manual: t('set by hand'), chapters: t('from a chapter named Credits in the file'), picture: t('found in the picture (dark screen with text near the end)'), sound: t('found by comparing the sound of the episodes'),
    none: t('none found'), pending: t('not looked for yet (detection runs in the background)') }[src] || src) + (errors ? t('; detection failed {n} time(s)', { n: errors }) : '');
  const markerPane = mk ? [t('Markers'), [
    h('p', { class: 'dim', style: 'margin-top:0' }, t('Intro: {from}.', { from: srcText(mk.introSource) }), h('br'), t('Credits: {from}. This video counts as watched at {at} of {length}.', { from: srcText(mk.source, mk.errors), at: fmtTime(mk.watchedAt / 1000), length: fmtTime(mk.duration / 1000) })),
    formRow(t('Intro starts at (hours:minutes:seconds)'), (mf.is = h('input', { value: mInit.is, placeholder: t('e.g. 0:00:45'), style: 'width:140px' }))),
    formRow(t('Intro ends at'), (mf.ie = h('input', { value: mInit.ie, placeholder: t('e.g. 0:02:10'), style: 'width:140px' })), t('Empty both to go back to the found intro.')),
    formRow(t('Credits start at'), (mf.cs = h('input', { value: mInit.cs, placeholder: t('e.g. 1:52:30'), style: 'width:140px' })), t('Empty to use the file’s chapters again.')),
    h('button', { title: t('Read the chapters of this file again and look for the end credits'), onclick: async () => {
      try { await api('POST', `/library/metadata/${it.id}/credits`); toast(t('Detecting credits…')); pollActivity(); } catch (e) { toast(e.message, 6000); }
    } }, t('Detect credits again'))]] : null;
  const panes = [
    [t('General'), [
      field('title', t('Title'), it.title), field('sort_title', t('Sort title'), it.titleSort, { help: t('Leave empty to derive it from the title.') }),
      field('original_title', t('Original title'), x.original_title),
      movie ? field('edition', t('Edition (e.g. Director’s Cut)'), x.edition) : null,
      field('originally_available', t('Originally available'), (x.originally_available || '').slice(0, 10), { type: 'date' }),
      field('year', t('Year'), it.year), field('content_rating', t('Age rating'), it.contentRating), field('rating', t('Rating (0-10)'), it.rating),
      field('studio', t('Studio'), it.studio), field('tagline', t('Tagline'), it.tagline), field('summary', t('Summary'), it.summary, { area: true })]],
    [t('Tags'), [
      field('directors', t('Directors'), list(x.directors)), field('countries', t('Countries'), list(x.countries)), field('genres', t('Genres'), list(it.genres)),
      field('writers', t('Writers'), list(x.writers)), field('producers', t('Producers'), list(x.producers)), field('collections', t('Collections'), list(x.collections))]],
    [t('Labels'), [field('labels', t('Labels (comma separated)'), list(x.labels))]],
    markerPane,
    [t('Advanced'), [
      formRow(t('Language for details'), (f.language_override = h('select', {}, h('option', { value: '' }, t('Library default')), langs.map(l => h('option', { value: l.code, selected: x.language_override === l.code }, l.name)))), t('Applies the next time the details are fetched online (menu "⋯" → refresh).')),
      formRow(t('Detect credits'), (f.credits_detection = h('select', {}, [['', t('Library default')], ['on', t('Yes')], ['off', t('No')]].map(([v, n]) => h('option', { value: v, selected: (x.credits_detection || '') === v }, n)))), t('Whether the "Skip credits" mark is searched for in this title.')),
      formRow(t('Use original title'), (f.use_original_title = h('select', {}, [['', t('Library default')], ['yes', t('Yes')], ['no', t('No')]].map(([v, n]) => h('option', { value: v, selected: (x.use_original_title || '') === v }, n)))), t('Shows the original title instead of the translated one.'))]],
    ...[['poster', t('Poster'), `/library/metadata/${it.id}/thumb`, true], ['art', t('Background'), `/library/metadata/${it.id}/art`, true], ['logo', t('Logo'), it.clearLogo, !!it.clearLogo], ['square', t('Square poster'), it.squareThumb, !!it.squareThumb]]
      .map(([slot, label, url, has]) => [label, [imagePicker(it, slot, url, has)]]),
    [t('Info'), [h('div', { class: 'kv' },
      h('div', { class: 'dim' }, 'ID'), h('div', {}, String(it.id)),
      h('div', { class: 'dim' }, t('Type')), h('div', {}, it.type),
      it.addedAt ? [h('div', { class: 'dim' }, t('Added')), h('div', {}, fmtAt(it.addedAt))] : null,
      it.updatedAt ? [h('div', { class: 'dim' }, t('Last updated')), h('div', {}, fmtAt(it.updatedAt))] : null,
      ((it.Media || [])[0] || {}).file ? [h('div', { class: 'dim' }, t('Path')), h('div', { style: 'word-break:break-all' }, it.Media[0].file)] : null)]]];
  for (let i = panes.length - 1; i >= 0; i--) if (!panes[i]) panes.splice(i, 1);
  panes.push(panes.splice(panes.findIndex(p => p[0] === t('Advanced')), 1)[0]); // Advanced last
  const bodies = panes.map(([, kids], i) => h('div', { style: i ? 'display:none' : '' }, kids));
  const links = panes.map(([name], i) => h('a', { href: '#', class: i ? '' : 'on', onclick: e => {
    e.preventDefault(); links.forEach((l, j) => { l.classList.toggle('on', j === i); bodies[j].style.display = j === i ? '' : 'none'; });
  } }, name));
  const box = modal(t('Edit details'), [
    h('div', { class: 'tabs' }, links), bodies,
    h('div', { class: 'actions' }, h('button', { class: 'primary', onclick: async () => {
      const body = {}; for (const [k, el] of Object.entries(f)) body[k] = el.value;
      body.year = body.year ? Number(body.year) : null; body.rating = body.rating ? Number(String(body.rating).replace(',', '.')) : null;
      if (!body.sort_title.trim()) delete body.sort_title;
      // Only markers that were changed are sent; emptied ones go back to what was found (only a hand-set one can be taken back).
      const mb = {};
      if (mk) {
        const cs = mf.cs.value.trim(), is = mf.is.value.trim(), ie = mf.ie.value.trim();
        if (cs !== mInit.cs) { if (cs) { const ms = unclock(cs); if (ms === null) return toast(t('Enter a time like 1:52:30')); mb.credits = [ms]; } else if (mk.source === 'manual') mb.credits = null; }
        if (is !== mInit.is || ie !== mInit.ie) {
          if (!is && !ie) { if (mk.introSource === 'manual') mb.intro = null; }
          else { const a = unclock(is), b = unclock(ie); if (a === null || b === null || b <= a) return toast(t('Enter the intro start and end, the end after the start')); mb.intro = [a, b]; }
        }
      }
      await api('PUT', '/library/metadata/' + it.id, body);
      if (Object.keys(mb).length) await api('PUT', `/library/metadata/${it.id}/markers`, mb);
      box.remove(); if (saved) saved(); else if (location.hash === '#/item/' + it.id) itemPage(it.id); else location.hash = '#/item/' + it.id;
    } }, t('Save')), h('button', { onclick: () => box.remove() }, t('Cancel')))]);
}

// Edit several titles at once. A field that is the same in all of them shows its value, one that
// differs says "Mixed"; only what is changed is saved. Lists (genres, directors, labels …) add and remove:
// what only some titles have stays unless it is taken out of the list here.
async function bulkEdit(picked, saved) {
  const its = await Promise.all(picked.map(fullItem));
  const langs = ((await GET('/api/settings').catch(() => null)) || {}).languages || [];
  const ex = it => it.extra || {};
  const f = {};
  const text = (k, label, get, o = {}) => {
    const vals = its.map(it => { const v = get(it); return v == null ? '' : String(v); });
    const mixed = vals.some(v => v !== vals[0]), init = mixed ? '' : vals[0];
    const el = h(o.area ? 'textarea' : 'input', { value: init, type: o.type || 'text', placeholder: mixed ? t('Mixed') : '', style: 'width:100%' });
    f[k] = { kind: 'text', el, init, num: o.num };
    return formRow(label, el, null, o.area);
  };
  const tags = (k, label, get) => {
    const lists = its.map(it => get(it) || []);
    const common = lists[0].filter(v => lists.every(l => l.includes(v)));
    const mixed = lists.some(l => l.length !== common.length);
    const el = h('input', { value: common.join(', '), placeholder: mixed ? t('Mixed') : '', style: 'width:100%' });
    f[k] = { kind: 'tags', el, common, get };
    return formRow(label, el, mixed ? t('What only some of the titles have stays as it is. Add it here to give it to all.') : null);
  };
  const choice = (k, label, opts, get) => {
    const vals = its.map(it => get(it) || ''), mixed = vals.some(v => v !== vals[0]), init = mixed ? '__mixed' : vals[0];
    const el = h('select', {}, [...(mixed ? [['__mixed', t('Mixed')]] : []), ...opts].map(([v, n]) => h('option', { value: v, selected: v === init }, n)));
    f[k] = { kind: 'text', el, init };
    return formRow(label, el);
  };
  const movies = its.every(it => it.type === 'movie' || it.type === 'episode');
  const panes = [
    [t('General'), [
      text('original_title', t('Original title'), it => ex(it).original_title),
      movies ? text('edition', t('Edition (e.g. Director’s Cut)'), it => ex(it).edition) : null,
      text('originally_available', t('Originally available'), it => (ex(it).originally_available || '').slice(0, 10), { type: 'date' }),
      text('year', t('Year'), it => it.year, { num: true }), text('content_rating', t('Age rating'), it => it.contentRating), text('rating', t('Rating (0-10)'), it => it.rating, { num: true }),
      text('studio', t('Studio'), it => it.studio), text('tagline', t('Tagline'), it => it.tagline), text('summary', t('Summary'), it => it.summary, { area: true })]],
    [t('Tags'), [
      tags('directors', t('Directors'), it => ex(it).directors), tags('countries', t('Countries'), it => ex(it).countries), tags('genres', t('Genres'), it => it.genres),
      tags('writers', t('Writers'), it => ex(it).writers), tags('producers', t('Producers'), it => ex(it).producers), tags('collections', t('Collections'), it => ex(it).collections)]],
    [t('Labels'), [tags('labels', t('Labels (comma separated)'), it => ex(it).labels)]],
    [t('Advanced'), [
      choice('language_override', t('Language for details'), [['', t('Library default')], ...langs.map(l => [l.code, l.name])], it => ex(it).language_override),
      choice('credits_detection', t('Detect credits'), [['', t('Library default')], ['on', t('Yes')], ['off', t('No')]], it => ex(it).credits_detection),
      choice('use_original_title', t('Use original title'), [['', t('Library default')], ['yes', t('Yes')], ['no', t('No')]], it => ex(it).use_original_title)]]];
  const bodies = panes.map(([, kids], i) => h('div', { style: i ? 'display:none' : '' }, kids));
  const links = panes.map(([name], i) => h('a', { href: '#', class: i ? '' : 'on', onclick: e => {
    e.preventDefault(); links.forEach((l, j) => { l.classList.toggle('on', j === i); bodies[j].style.display = j === i ? '' : 'none'; });
  } }, name));
  const split = s => s.split(',').map(x => x.trim()).filter(Boolean);
  // What to send to each title: only the fields that were changed.
  const bodyFor = it => {
    const body = {};
    for (const [k, c] of Object.entries(f)) {
      if (c.kind === 'tags') {
        const now = split(c.el.value), add = now.filter(v => !c.common.includes(v)), del = c.common.filter(v => !now.includes(v));
        if (!add.length && !del.length) continue;
        body[k] = [...(c.get(it) || []).filter(v => !del.includes(v)), ...add.filter(v => !(c.get(it) || []).includes(v))];
      } else if (c.el.value !== c.init && c.el.value !== '__mixed') {
        const v = c.el.value;
        body[k] = c.num ? (v === '' ? null : Number(String(v).replace(',', '.'))) : v;
      }
    }
    return body;
  };
  const box = modal(t('Edit {n} titles', { n: its.length }), [
    h('p', { class: 'dim', style: 'margin-top:0' }, t('Only what you change is saved. A field that differs between the titles says “Mixed”.')),
    h('div', { class: 'tabs' }, links), bodies,
    h('div', { class: 'actions' }, h('button', { class: 'primary', onclick: async e => {
      const jobs = its.map(it => [it, bodyFor(it)]).filter(([, b]) => Object.keys(b).length);
      if (!jobs.length) { box.remove(); return toast(t('Nothing changed')); }
      e.target.disabled = true;
      try { for (const [it, b] of jobs) await api('PUT', '/library/metadata/' + it.id, b); }
      catch (err) { e.target.disabled = false; return toast(err.message); }
      box.remove(); toast(t('{n} titles saved', { n: jobs.length })); if (saved) saved();
    } }, t('Save')), h('button', { onclick: () => box.remove() }, t('Cancel')))]);
}

// ---------- settings kept in this browser, and those that follow the account from browser to browser
const store = {
  get: (k, d) => { try { const v = localStorage.getItem(k); return v === null ? d : v; } catch (e) { return d; } },
  set: (k, v) => { try { localStorage.setItem(k, String(v)); } catch (e) { /* private window */ } if (SYNCED.includes(k)) syncPref(k, String(v)); },
};
// The video codecs this browser plays besides H.264 (Chrome with a hardware decoder plays HEVC): sent along
// so the server does not convert what the browser can show itself.
const CLIENT_CODECS = (() => {
  const v = document.createElement('video'), ok = type => { try { return !!v.canPlayType(type); } catch (e) { return false; } };
  return [ok('video/mp4; codecs="hvc1.1.6.L153.B0"') ? 'hevc' : '', ok('video/webm; codecs="vp9"') ? 'vp9' : '', ok('video/mp4; codecs="av01.0.08M.08"') ? 'av1' : ''].filter(Boolean).join(',');
})();
// Audio, subtitle and watched-mark settings follow the account from browser to browser: they are kept on the
// server too (POST /api/me/prefs); at start the server's values win, and values only this browser has go up once.
const SYNCED = ['audioLang', 'subLang', 'subMode', 'subPrefLang', 'subSdh', 'subForced', 'subSize', 'subColor', 'subBg', 'subLift', 'burnSubs', 'watchedMark',
  'theme', 'accent', 'timeFormat', 'cardSize', 'dlQuality', 'navMore', 'quality', 'directPlay', 'keepSmall', 'levelVolume', 'themeMusic', 'rememberTab', 'autoNext', 'timeLeft', 'speed', 'homeLayout', 'nightMode'];
let prefQueue = {}, prefTimer = null;
function syncPref(k, v) {
  if (!S.me) return;
  prefQueue[k] = v; clearTimeout(prefTimer);
  prefTimer = setTimeout(() => { const q = prefQueue; prefQueue = {}; api('POST', '/api/me/prefs', q).catch(() => {}); }, 500);
}
function pullPrefs() {
  const server = (S.me && S.me.prefs) || {}, up = {};
  for (const k of SYNCED) {
    if (k in server) { try { localStorage.setItem(k, server[k]); } catch (e) { /* none */ } }
    else { const v = store.get(k, null); if (v !== null) up[k] = v; }
  }
  // The app's language travels along too (a changed language reloads the page once).
  let mine = null; try { mine = localStorage.getItem('lang'); } catch (e) { /* none */ }
  if (server.lang && server.lang !== mine && I18N.packs[server.lang]) { try { localStorage.setItem('lang', server.lang); } catch (e) { /* none */ } location.reload(); return; }
  if (!server.lang && mine) up.lang = mine;
  if (Object.keys(up).length) api('POST', '/api/me/prefs', up).catch(() => {});
  window.applyTheme();
  // What the player read when this file loaded.
  P.quality = store.get('quality', 'original'); P.speed = Number(store.get('speed', 1)) || 1;
  P.autoNext = store.get('autoNext', '1') === '1'; P.remaining = store.get('timeLeft', '0') === '1';
  LEVEL.night = store.get('nightMode', '0') === '1';
  cardSize();
}
window.saveLangPref = code => (S.me ? api('POST', '/api/me/prefs', { lang: code }).catch(() => {}) : null);
