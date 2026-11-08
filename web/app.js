// Lichtspiel web app, part 1: helpers, sign-in, browsing, details, player.
// Talks only to the server's JSON API (see #/api). Part 2 is pages.js.
// Every text shown goes through t() (i18n.js); the language packs are in web/lang/.
'use strict';
// A link with ?token=… (made by the status window and the self-test) signs this browser in once,
// then the token is taken out of the address bar.
(() => {
  const u = new URL(location.href); const tok = u.searchParams.get('token');
  if (tok) { try { localStorage.setItem('token', tok); } catch (e) { /* none */ } u.searchParams.delete('token'); history.replaceState(null, '', u.pathname + u.search + u.hash); }
})();
// Where the sign-in is kept: for good (the default), or only until the browser is closed
// (Settings -> Web -> sign in automatically: off).
const keepSignedIn = () => { try { return localStorage.getItem('autoLogin') !== '0'; } catch (e) { return true; } };
function saveToken(tok) {
  try {
    if (keepSignedIn()) { localStorage.setItem('token', tok); sessionStorage.removeItem('token'); }
    else { sessionStorage.setItem('token', tok); localStorage.removeItem('token'); }
  } catch (e) { /* private window: the sign-in lasts for this page */ }
}
function loadToken() { try { return sessionStorage.getItem('token') || localStorage.getItem('token') || ''; } catch (e) { return ''; } }
const S = { token: loadToken(), me: null, sections: [], filters: null, status: null };
const $ = (s, r = document) => r.querySelector(s);
const main = () => $('#main');

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
  return h('div', { class: 'field' + (top ? ' top' : '') }, h('label', control && control.id ? { for: control.id } : {}, label),
    h('div', { class: 'ctl' }, control), help ? h('div', { class: 'help dim' }, help) : null);
}
// The management pages (libraries, users, …) are tabs of the settings and show their tab bar.
const MANAGE_PAGES = ['libraries', 'users', 'plugins', 'reports', 'api'];
function set(...kids) {
  const list = kids.flat(9).filter(Boolean);
  if (typeof NAV !== 'undefined' && MANAGE_PAGES.includes(NAV.cur) && S.me && S.me.admin) list.unshift(settingsNav(NAV.cur));
  // The tab bars scroll sideways on a phone: keep where they were, and keep the chosen tab in view.
  const kept = [...main().querySelectorAll('.tabs')].map(e => e.scrollLeft);
  main().replaceChildren(...list); window.scrollTo(0, 0);
  main().querySelectorAll('.tabs').forEach((bar, i) => {
    if (kept[i]) bar.scrollLeft = kept[i];
    const on = bar.querySelector('a.on'); if (!on || bar.scrollWidth <= bar.clientWidth) return;
    const b = bar.getBoundingClientRect(), o = on.getBoundingClientRect();
    if (o.left < b.left || o.right > b.right) bar.scrollLeft += o.left - b.left - (b.width - o.width) / 2;
  });
}
function toast(msg, ms = 2600) { const el = $('#toast'); el.textContent = msg; el.style.display = 'block'; clearTimeout(el._t); el._t = setTimeout(() => (el.style.display = 'none'), ms); }
const withTok = p => p + (p.includes('?') ? '&' : '?') + 'token=' + encodeURIComponent(S.token);
const fmtNum = (n, digits) => Number(n).toLocaleString(I18N.lang, digits === undefined ? undefined : { minimumFractionDigits: digits, maximumFractionDigits: digits });
const fmtDur = ms => { if (!ms) return ''; const m = Math.round(ms / 60000); if (m < 1) return Math.max(1, Math.round(ms / 1000)) + ' s'; return m >= 60 ? t('{h} h {m} min', { h: Math.floor(m / 60), m: m % 60 }) : t('{m} min', { m }); };
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
  const opt = { method, headers: { 'X-Token': S.token, Accept: 'application/json' } };
  if (body !== undefined) { opt.headers['Content-Type'] = 'application/json'; opt.body = JSON.stringify(body); }
  const r = await fetch(path, opt);
  let j = null;
  try { j = await r.json(); } catch (e) { /* not JSON */ }
  if (r.status === 401 && S.token && !path.startsWith('/api/login')) { logout(); throw new Error(t('Please sign in again')); }
  if (!r.ok) { const e = new Error((j && j.error) ? t(j.error) : 'HTTP ' + r.status); e.data = j; throw e; }
  return j;
}
const GET = p => api('GET', p);
const meta = j => (j && j.MediaContainer && j.MediaContainer.Metadata) || [];

// ---------- sign in, first setup, invites
function logout() { S.token = ''; S.me = null; try { localStorage.removeItem('token'); sessionStorage.removeItem('token'); } catch (e) { /* none */ } location.hash = '#/login'; drawChrome(); render(); }

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
  location.hash = '#/';
  try { await boot(); } finally { S.signingIn = false; }
  if (S.me) toast(t('Signed in as {name}', { name: S.me.username }));
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
  // Home profiles (only offered in the local network): a tap, and the PIN if there is one.
  const form = main().querySelector('form.center');
  let home;
  try { home = await api('GET', '/api/home'); } catch (e) { return; }
  if (!home.profiles.length || !form.isConnected) return;
  const pick = h('div', { class: 'profiles' }, home.profiles.map(p => h('button', { type: 'button', class: 'profile', title: p.username, onclick: () => pinPage(p) },
    h('span', { class: 'avatar' }, (p.username[0] || '?').toUpperCase()),
    h('span', { class: 'pname' }, p.username, p.has_pin ? h('span', { class: 'dim', title: t('Needs a PIN') }, ' 🔒') : null))));
  const or = h('div', { class: 'or dim' }, t('or sign in with a password'));
  const firstLabel = form.querySelector('label');
  form.insertBefore(pick, firstLabel);
  form.insertBefore(or, firstLabel);
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
    h('div', { class: 'profiles' }, h('div', { class: 'profile' }, h('span', { class: 'avatar' }, (p.username[0] || '?').toUpperCase()), h('span', { class: 'pname' }, p.username))),
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
  const [status, me, secs, filters] = await Promise.all([GET('/api/status'), GET('/api/me'), GET('/library/sections'), GET('/api/filters')].map(p => p.catch(() => null)));
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
}
function drawChrome() {
  const who = $('#who');
  who.replaceChildren();
  const signed = !!S.me;
  $('#side').style.display = signed ? '' : 'none';
  $('#search').style.visibility = signed ? '' : 'hidden';
  $('#main').style.marginLeft = signed && innerWidth > 760 ? '220px' : '0';
  if (!signed) return;
  // The gear opens the settings; switching user and signing out sit behind the name.
  const gear = iconBtn('gear', t('Settings'), () => { location.hash = '#/settings'; });
  const name = h('button', { class: 'link', 'aria-haspopup': 'menu', title: t('Switch user or sign out'), style: 'background:none;border:0;color:inherit;font:inherit;cursor:pointer;padding:4px 6px' }, S.me.username + ' ▾');
  name.onclick = () => openMenu(name, [
    { label: t('Switch user'), run: switchUser },
    { label: t('Sign out'), run: logout },
  ]);
  who.append(name, gear);
  document.querySelectorAll('#side .admin').forEach(e => (e.style.display = S.me.admin ? '' : 'none'));
  $('#npbtn').style.display = S.me.admin ? '' : 'none';
  drawQueueChip();
  drawNav();
  drawDlChip();
}
function markNav(key) { NAV.cur = key; document.querySelectorAll('#side a').forEach(a => a.classList.toggle('on', a.dataset.nav === key)); $('#npbtn').classList.toggle('on', key === 'dashboard'); }
let searchTimer;
$('#q').addEventListener('input', e => { clearTimeout(searchTimer); searchTimer = setTimeout(() => { const v = e.target.value.trim(); location.hash = v ? '#/search/' + encodeURIComponent(v) : '#/'; }, 300); });

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
  const more = e => {
    e.stopPropagation();
    // After a change (match, refresh, unmatch, watched …) the card is redrawn with what the server has now.
    itemMenu(it, e.currentTarget, async () => {
      try { Object.assign(it, meta(await GET('/library/metadata/' + it.id))[0]); } catch (err) { /* keep what we have */ }
      const fresh = posterCard(it, opts); card.replaceWith(fresh); if (opts.onReplace) opts.onReplace(fresh);
    }, { cont: opts.cont, removed: () => card.remove() });
  };
  // The pencil (admins) edits the details right here; the card is redrawn with what was saved.
  const edit = async e => {
    e.stopPropagation();
    editItem(await fullItem(it), async () => {
      const fresh = posterCard(Object.assign(it, meta(await GET('/library/metadata/' + it.id))[0]), opts);
      card.replaceWith(fresh); if (opts.onReplace) opts.onReplace(fresh);
    });
  };
  const card = h('div', { class: 'card' + (seen ? ' watched' : '') + (it.missing ? ' missing' : '') + (['artist', 'album', 'track'].includes(it.type) ? ' music' : ''), onauxclick: e => { if (e.button === 1) { e.preventDefault(); window.open(location.pathname + '#/item/' + it.id, '_blank'); } }, onmousedown: e => { if (e.button === 1) e.preventDefault(); }, onclick: e => { if (!(opts.pick && opts.pick(e, false))) location.hash = '#/item/' + it.id; } },
    h('div', { class: 'poster', 'data-id': it.id }, h('img', { src: withTok(img + (img.includes('?') ? '&' : '?') + 'w=360'), loading: 'lazy', alt: '' }),
      it.missing ? h('span', { class: 'gone', title: t('The file is gone from disk; the item is in the library’s trash'), 'aria-label': t('Unavailable') }, icon('trash')) : null,
      prog > 0 ? h('div', { class: 'bar' }, h('i', { style: `width:${prog * 100}%` })) : null,
      it.leafCount && unseen > 0 ? h('span', { class: 'badge' }, unseen) : null,
      seen ? h('span', { class: 'seen', title: t('Watched'), 'aria-label': t('Watched') }, icon('check')) : null,
      opts.pick ? h('span', { class: 'pick', title: t('Select'), onclick: e => { e.stopPropagation(); opts.pick(e, true); } }) : null,
      isPlayable(it) && !it.missing ? h('button', { class: 'play', title: t('Play'), 'aria-label': t('Play'), onclick: e => { e.stopPropagation(); play(it, (it.viewOffset || 0) / 1000); } }, icon('play')) : null,
      S.me.admin && it.type !== 'clip' ? h('button', { class: 'edit', title: t('Edit details'), 'aria-label': t('Edit details'), onclick: edit }, icon('edit')) : null,
      it.type === 'clip' ? null : h('button', { class: 'more', title: t('More'), 'aria-label': t('More'), onclick: more }, '⋯')),
    h('div', { class: 't', title }, title), h('div', { class: 's' }, (opts.caption && opts.caption(it)) || sub));
  return card;
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
    await api('POST', `/:/${watched ? 'scrobble' : 'unscrobble'}?key=${ids().join(',')}`);
    for (const i of idx) {
      const it = items[i];
      if (it.leafCount) it.viewedLeafCount = watched ? it.leafCount : 0;
      it.viewCount = watched ? Math.max(1, it.viewCount || 0) : 0;
      it.viewOffset = 0;
      const fresh = card(i); cards[i].replaceWith(fresh); cards[i] = fresh;
    }
    toast(watched ? t('{n} marked as watched', { n: idx.length }) : t('{n} marked as unwatched', { n: idx.length }));
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
          const fresh = card(i); cards[i].replaceWith(fresh); cards[i] = fresh;
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
    if (!circle && !picked.size && !e.ctrlKey && !e.metaKey && !e.shiftKey) return false;
    e.preventDefault(); toggle(i, e); return true;
  } });
  const card = i => { const c = card0(i); c.dataset.i = i; return c; };
  const cards = [];
  // More items arrive while scrolling.
  grid._add = list => { for (const it of list) { const i = items.length; items.push(it); cards.push(card(i)); grid.append(cards[i]); } };
  const firstItems = items.splice(0); grid._add(firstItems);
  grid._clear = () => { if (!picked.size) return false; clear(); return true; };
  // Empty cards for titles that are not loaded yet (items[i] is null until _put fills it).
  grid._reserve = n => {
    const music = cards[0] && cards[0].classList.contains('music');
    for (let k = 0; k < n; k++) {
      const i = items.length; items.push(null);
      const ph = h('div', { class: 'card ph' + (music ? ' music' : ''), 'data-i': i }, h('div', { class: 'poster' }), h('div', { class: 't' }, '\u00a0'), h('div', { class: 's' }, '\u00a0'));
      cards.push(ph); grid.append(ph);
    }
  };
  grid._put = (start, list) => list.forEach((it, k) => {
    const i = start + k;
    if (i < items.length && !items[i]) { items[i] = it; const c = card(i); cards[i].replaceWith(c); cards[i] = c; }
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
// Grey placeholders while a page's rows are still on their way.
const skeletonRows = n => Array.from({ length: n }, () => [h('div', { class: 'skel skel-h' }),
  h('div', { class: 'row' }, Array.from({ length: 10 }, () => h('div', { class: 'card' }, h('div', { class: 'poster skel' }), h('div', { class: 'skel skel-t' }))))]);
async function homePage() {
  markNav('home');
  // Answer the click at once; the rows come when the server has them.
  if (S.sections.length) set(h('h1', {}, t('Home')), skeletonRows(2));
  const hubs = (await GET('/hubs')).MediaContainer.Hub;
  if (!S.sections.length) {
    return set(h('h1', {}, t('Welcome')), h('div', { class: 'empty' },
      S.me.admin ? [h('p', {}, t('No libraries yet. Add your movie and TV show folders to get started.')), h('button', { class: 'primary', onclick: () => (location.hash = '#/libraries') }, t('Add a library'))]
        : t('The owner has not shared any libraries with you yet.')));
  }
  set(h('h1', {}, t('Home')), hubs.length ? hubs.map(hb => row(hb.title, hb.Metadata)) : h('div', { class: 'empty' }, t('Nothing here yet. Libraries are still being scanned, or they are empty.')));
}
async function searchPage(q) {
  markNav('');
  const r = meta(await GET('/hubs/search?query=' + encodeURIComponent(q)));
  set(h('h1', {}, t('Search: {q}', { q })), r.length ? h('div', { class: 'grid' }, r.map(i => posterCard(i))) : h('div', { class: 'empty' }, t('Nothing found.')));
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

// ---------- a library's page: filters, sorting, letters and pages that load while you scroll

// Sort fields of a library; the direction is a switch of its own.
const SORT_FIELDS = [['title', 'Title'], ['year', 'Year'], ['release_date', 'Release date'], ['critic_rating', 'Critic rating'], ['audience_rating', 'Audience rating'],
  ['user_rating', 'Your rating'], ['content_rating', 'Age rating'], ['duration', 'Length'], ['progress', 'Progress'], ['view_count', 'Plays'], ['added_at', 'Date added'],
  ['last_viewed_at', 'Date played'], ['resolution', 'Resolution'], ['bitrate', 'Bitrate'], ['random', 'Random']];
const SORT_ASC = new Set(['title', 'show', 'content_rating', 'random']);
const parseSort = s => { const [f, d] = String(s || 'title').split(':'); return { field: f || 'title', desc: d ? d === 'desc' : !SORT_ASC.has(f) }; };
const sortName = f => t(f === 'show' ? 'TV show' : (SORT_FIELDS.find(x => x[0] === f) || [f, f])[1]);

// "5 days ago", with the exact date as the tooltip.
function relTime(sec) {
  const d = Math.round((Date.now() / 1000 - sec) / 86400);
  const rtf = new Intl.RelativeTimeFormat(I18N.lang, { numeric: 'auto' });
  if (d < 1) return rtf.format(0, 'day');
  if (d < 14) return rtf.format(-d, 'day');
  if (d < 60) return rtf.format(-Math.round(d / 7), 'week');
  if (d < 730) return rtf.format(-Math.round(d / 30.4), 'month');
  return rtf.format(-Math.round(d / 365), 'year');
}
const SCORE_SRC = { imdb: 'IMDb', tmdb: 'TMDB', rotten_tomatoes: 'RT', metacritic: 'MC' };
// What a poster says under its title, by the way the library is sorted.
function captionFor(field) {
  const num = v => fmtNum(v, 1);
  switch (field) {
    case 'duration': return it => (it.duration ? fmtDur(it.duration) : '');
    case 'release_date': return it => (it.originallyAvailableAt ? new Date(it.originallyAvailableAt).toLocaleDateString(I18N.lang) : it.year || '');
    case 'audience_rating': return it => { const s = it.scores || {}; const k = ['imdb', 'tmdb'].find(x => s[x]); return k ? h('span', {}, h('b', { class: 'src' }, SCORE_SRC[k]), ' ', String(s[k]).replace('/10', '')) : (it.rating ? num(it.rating) : ''); };
    case 'critic_rating': return it => { const s = it.scores || {}; const k = ['rotten_tomatoes', 'metacritic'].find(x => s[x]); return k ? h('span', {}, h('b', { class: 'src' }, SCORE_SRC[k]), ' ', s[k]) : ''; };
    case 'user_rating': return it => (it.userRating ? '★'.repeat(Math.round(it.userRating / 2)) : '');
    case 'content_rating': return it => it.contentRating || '';
    case 'progress': return it => (it.viewOffset && it.duration ? Math.round(100 * it.viewOffset / it.duration) + ' %' : '');
    case 'view_count': return it => (it.viewCount ? t('{n} plays', { n: it.viewCount }) : '');
    case 'added_at': return it => (it.addedAt ? h('span', { title: new Date(it.addedAt * 1000).toLocaleString(I18N.lang) }, relTime(it.addedAt)) : '');
    case 'last_viewed_at': return it => (it.lastViewedAt ? h('span', { title: new Date(it.lastViewedAt * 1000).toLocaleString(I18N.lang) }, relTime(it.lastViewedAt)) : '');
    case 'resolution': return it => { const m = it.Media && it.Media[0]; return m && m.height ? (m.height >= 2000 ? '4K' : m.height + 'p') : ''; };
    case 'bitrate': return it => { const m = it.Media && it.Media[0]; return m && m.bitrate ? t('{n} Mbit/s', { n: num(m.bitrate / 1000) }) : ''; };
    default: return null;
  }
}

// The filters of the library menu, grouped: states first, then the lists of values.
const FILTER_GROUP_STATE = ['unwatched', 'unwatched_leaves', 'in_progress', 'unmatched', 'duplicate'];
const FILTER_GROUP_MAIN = ['genre', 'year', 'decade', 'content_rating', 'collection', 'director', 'actor', 'writer', 'producer', 'country', 'network', 'studio'];
// Operators of the advanced filters, in plain words.
const ADV_OPS = {
  text: [['contains', 'contains'], ['not_contains', 'does not contain'], ['is', 'is'], ['is_not', 'is not'], ['starts_with', 'begins with'], ['ends_with', 'ends with']],
  number: [['is', 'is'], ['is_not', 'is not'], ['gt', 'is greater than'], ['lt', 'is less than']],
  date: [['gt', 'is after'], ['lt', 'is before'], ['in_last_days', 'is within the last … days'], ['not_in_last_days', 'is not within the last … days']],
  bool: [['is', 'applies']],
};
const ADV_FIELDS = ['title', 'year', 'genre', 'content_rating', 'studio', 'summary', 'added_at', 'release_date', 'last_viewed_at', 'duration', 'audience_rating', 'critic_rating', 'user_rating',
  'view_count', 'progress', 'resolution', 'bitrate', 'size', 'container', 'video_codec', 'audio_codec', 'audio_language', 'subtitle_language', 'director', 'actor', 'writer', 'producer',
  'country', 'collection', 'label', 'edition', 'folder', 'unwatched', 'in_progress', 'unmatched', 'duplicate', 'hdr', 'dovi', 'favorite', 'watchlist'];
const ADV_NAMES = { title: 'Title', summary: 'Description', added_at: 'Date added', release_date: 'Release date', last_viewed_at: 'Date played', duration: 'Length (minutes)', audience_rating: 'Audience rating',
  critic_rating: 'Critic rating', user_rating: 'Your rating', view_count: 'Plays', progress: 'Progress (%)', resolution: 'Picture height (pixels)', bitrate: 'Bitrate (kbit/s)', size: 'File size (MB)',
  container: 'File type', favorite: 'Favorite', watchlist: 'On the watchlist', unwatched: 'Unwatched', unmatched: 'Unmatched' };
const advName = f => t(ADV_NAMES[f] || FILTER_NAMES[f] || f);

// Older links (and bookmarks) used ?genre= and ?watched=; they become filters.
const OLD_WATCHED = { unwatched: ['unwatched', 'true'], watched: ['unwatched', 'false'], progress: ['in_progress', 'true'] };
const PAGE_SIZE = 120;
// Where each library page was scrolled to (kept while the tab lives), so "back" lands where you left.
const LIBPOS = (() => { try { return JSON.parse(sessionStorage.getItem('libpos') || '{}'); } catch (e) { return {}; } })();
const saveLibPos = () => { try { sessionStorage.setItem('libpos', JSON.stringify(LIBPOS)); } catch (e) { /* none */ } };
// The place in a library is for coming back (Back from a title). Clicking its link (side bar, tab) opens it at the top.
document.addEventListener('click', e => {
  const a = e.target.closest && e.target.closest('a[href^="#/section/"]');
  if (a) delete LIBPOS[a.getAttribute('href')];
}, true);

// The filter menu of the library pages (also used by the watchlist): the button, the panel with the kinds of filter and their
// values, the advanced rules and the chips of what is on. st holds the state (f.<name> = value, rules, rmatch), go() applies it.
function filterUI({ st, go, advRules, lastKey, loadFilters }) {
  const filterName = f => t(FILTER_NAMES[f] || f);
  const valueName = (f, v) => f === 'video_resolution' ? (RES_NAMES[v] || v) : f === 'decade' ? t('{v}s', { v }) : /_language$/.test(f) ? (langName(v) || v) : v;
  const setFilter = (f, v) => { if (v) st['f.' + f] = v; else delete st['f.' + f]; go(); };
  // ----- the filter panel: the kinds of filter on the left, the values of the chosen one (with a search box) on the right
  let filters = null;
  let panel = null;
  const closePanel = () => { if (panel) { panel.remove(); panel = null; document.removeEventListener('mousedown', outside, true); } filterBtn.classList.remove('open'); };
  const outside = e => { if (panel && !panel.contains(e.target) && !filterBtn.contains(e.target) && !document.querySelector('.modal')) closePanel(); };
  const openPanel = async () => {
    if (panel) return closePanel();
    try { filters = filters || await loadFilters(); }
    catch (e) { return toast(e.message, 6000); }
    const byName = Object.fromEntries(filters.map(f => [f.filter, f]));
    const states = FILTER_GROUP_STATE.map(k => byName[k]).filter(Boolean);
    const main = FILTER_GROUP_MAIN.map(k => byName[k]).filter(Boolean);
    const more = filters.filter(f => !states.includes(f) && !main.includes(f));
    let cur = ''; try { cur = localStorage.getItem(lastKey) || ''; } catch (e) { /* none */ }
    if (!byName[cur] || byName[cur].type === 'boolean') cur = (main[0] || more[0] || {}).filter || '';
    const right = h('div', { class: 'fp-right' }), left = h('div', { class: 'fp-left' });
    const drawRight = () => {
      const f = byName[cur];
      if (!f) return right.replaceChildren(h('div', { class: 'dim' }, t('None found')));
      const on = st['f.' + f.filter] || '';
      const search = h('input', { type: 'search', placeholder: t('Search {name}…', { name: filterName(f.filter) }), style: 'width:100%' });
      const list = h('div', { class: 'fp-values' });
      const fill = () => {
        const needle = search.value.trim().toLowerCase();
        const vals = f.values.filter(x => !needle || valueName(f.filter, x.value).toLowerCase().includes(needle));
        list.replaceChildren(...(vals.length ? vals.map(x => h('button', { class: 'fp-val' + (x.value === on ? ' on' : ''), onclick: () => setFilter(f.filter, x.value === on ? '' : x.value) },
          h('span', {}, valueName(f.filter, x.value)), h('small', { class: 'dim' }, String(x.count)))) : [h('div', { class: 'dim', style: 'padding:8px' }, t('None found'))]));
      };
      search.oninput = fill; fill();
      right.replaceChildren(search, list);
      setTimeout(() => search.focus(), 0);
    };
    const drawLeft = () => {
      const entry = f => {
        const val = st['f.' + f.filter] || '';
        if (f.type === 'boolean') return h('button', { class: 'fp-item' + (val ? ' on' : ''), onclick: () => setFilter(f.filter, val ? '' : 'true') }, h('span', {}, filterName(f.filter)), h('small', { class: 'dim' }, val ? '✓' : f.count != null ? String(f.count) : ''));
        return h('button', { class: 'fp-item' + (f.filter === cur ? ' cur' : '') + (val ? ' on' : ''), onclick: () => { cur = f.filter; try { localStorage.setItem(lastKey, cur); } catch (e) { /* none */ } drawLeft(); drawRight(); } },
          h('span', {}, filterName(f.filter)), h('small', { class: 'dim' }, val ? valueName(f.filter, val).slice(0, 14) : '›'));
      };
      left.replaceChildren(...states.map(entry), states.length ? h('hr') : '', ...main.map(entry), more.length ? h('hr') : '', ...more.map(entry),
        h('hr'), h('button', { class: 'fp-item', onclick: () => { closePanel(); advancedDialog(); } }, h('span', {}, t('Advanced filters…')), h('small', { class: 'dim' }, advRules.length ? String(advRules.length) : '')));
    };
    drawLeft(); drawRight();
    panel = h('div', { class: 'filterpanel' }, left, right);
    const r = filterBtn.getBoundingClientRect();
    panel.style.left = Math.max(8, Math.min(r.left, innerWidth - 620)) + 'px'; panel.style.top = (r.bottom + 6) + 'px';
    document.body.append(panel);
    filterBtn.classList.add('open');
    document.addEventListener('mousedown', outside, true);
  };
  // ----- advanced filters: "all / any of the following" with a field, an operator and a value per line
  const advancedDialog = () => {
    const rules = JSON.parse(JSON.stringify(advRules.length ? advRules : [{ field: 'title', op: 'contains', value: '' }]));
    let any = st.rmatch === 'any';
    const fieldType = f => { const def = (S.filters.fields || []).find(x => x.field === f); const ty = def ? def.type : 'text'; return ty === 'sort' ? 'text' : ty; };
    const box0 = h('div');
    const dayOf = v => (v ? new Date(v * 1000).toISOString().slice(0, 10) : '');
    const draw = () => box0.replaceChildren(...rules.map((r, i) => {
      const ty = fieldType(r.field), ops = ADV_OPS[ty] || ADV_OPS.text;
      if (!ops.some(o => o[0] === r.op)) r.op = ops[0][0];
      let val;
      if (ty === 'bool') val = h('select', { onchange: e => { r.value = e.target.value === 'true'; } }, h('option', { value: 'true', selected: r.value !== false }, t('yes')), h('option', { value: 'false', selected: r.value === false }, t('no')));
      else if (ty === 'date' && !r.op.includes('days')) val = h('input', { type: 'date', value: dayOf(r.value), onchange: e => { r.value = e.target.value ? Math.floor(new Date(e.target.value + 'T00:00:00Z').getTime() / 1000) : ''; } });
      else val = h('input', { value: r.value ?? '', type: ty === 'number' || r.op.includes('days') ? 'number' : 'text', placeholder: r.op.includes('days') ? t('days') : t('value'), onchange: e => { r.value = e.target.value; } });
      if (ty === 'bool' && r.value === '') r.value = true;
      return h('div', { class: 'rule' },
        h('select', { onchange: e => { r.field = e.target.value; const nt = fieldType(r.field); r.op = (ADV_OPS[nt] || ADV_OPS.text)[0][0]; r.value = nt === 'bool' ? true : ''; draw(); } },
          ADV_FIELDS.map(f => h('option', { value: f, selected: f === r.field }, advName(f)))),
        ty === 'bool' ? null : h('select', { onchange: e => { r.op = e.target.value; r.value = ''; draw(); } }, ops.map(([o, l]) => h('option', { value: o, selected: o === r.op }, t(l)))),
        val, h('button', { class: 'small danger', title: t('Remove'), onclick: () => { rules.splice(i, 1); draw(); } }, '✕'));
    }));
    const box = modal(t('Advanced filters'), [
      formRow(t('Show items that match'), h('select', { onchange: e => { any = e.target.value === 'any'; } },
        h('option', { value: 'all', selected: !any }, t('all of the following')), h('option', { value: 'any', selected: any }, t('any of the following')))),
      box0, h('button', { class: 'small', onclick: () => { rules.push({ field: 'genre', op: 'is', value: '' }); draw(); } }, t('+ Add rule')),
      h('div', { class: 'actions' },
        h('button', { class: 'primary', onclick: () => {
          const ok = rules.filter(r => r.value !== '' && r.value !== undefined && r.value !== null);
          st.rules = ok.length ? JSON.stringify(ok.map(r => ({ field: r.field, op: r.op, value: r.value }))) : ''; st.rmatch = any && ok.length ? 'any' : ''; box.remove(); go();
        } }, t('Apply')),
        advRules.length ? h('button', { onclick: () => { st.rules = ''; st.rmatch = ''; box.remove(); go(); } }, t('Remove all')) : null,
        h('button', { onclick: () => box.remove() }, t('Cancel')))]);
    draw();
  };
  const active = Object.keys(st).filter(k => k.startsWith('f.') && st[k]);
  const nActive = active.length + (advRules.length ? 1 : 0);
  const filterBtn = h('button', { class: 'dd' + (nActive ? ' on' : ''), 'aria-haspopup': 'true', onclick: openPanel }, icon('filter'), h('span', {}, nActive ? t('Filter ({n})', { n: nActive }) : t('Filter')), icon('down'));
  const chips = nActive ? h('div', { class: 'chips' }, active.map(k => {
    const f = k.slice(2), v = st[k];
    const label = v === 'true' ? filterName(f) : v === 'false' ? t('{name}: no', { name: filterName(f) }) : `${filterName(f)}: ${valueName(f, v)}`;
    return h('button', { class: 'chip', title: t('Remove this filter'), onclick: () => setFilter(f, '') }, label, h('span', { 'aria-hidden': 'true' }, '×'));
  }), advRules.length ? h('button', { class: 'chip', title: t('Advanced filters'), onclick: advancedDialog }, t('Advanced filters ({n})', { n: advRules.length })) : null,
    nActive > 1 ? h('button', { class: 'small', onclick: () => { for (const k of active) delete st[k]; st.rules = ''; st.rmatch = ''; go(); } }, t('Clear filters')) : null) : null;
  return { filterBtn, chips, closePanel };
}

async function sectionPage(id, qs) {
  markNav('section/' + id);
  const myHash = location.hash;
  const sec = S.sections.find(s => String(s.id) === String(id));
  if (!sec) return set(h('div', { class: 'empty' }, t('Library not found.')));
  let p = new URLSearchParams(qs || '');
  // Opened without a choice: the filters and sorting of the last visit come back.
  if (!qs) { try { const last = localStorage.getItem('libstate:' + id); if (last) p = new URLSearchParams(last); } catch (e) { /* none */ } }
  // The library's own pages: Library, Collections, Playlists, Categories.
  const tab = p.get('tab') || '';
  const tabs = h('div', { class: 'tabs' }, [['', t('Library')], ['collections', t('Collections')], ['playlists', t('Playlists')], ['categories', t('Categories')]]
    .map(([k, l]) => h('a', { href: `#/section/${id}${k ? '?tab=' + k : ''}`, class: tab === k ? 'on' : '' }, l)));
  if (tab) return sectionSubPage(sec, tab, tabs);
  const st = {}; for (const k of ['type', 'sort', 'from', 'to', 'q', 'container', 'rules', 'rmatch']) st[k] = p.get(k) || '';
  for (const [k, v] of p) if (k.startsWith('f.')) st[k] = v;
  if (p.get('genre')) st['f.genre'] = p.get('genre');
  if (p.get('collection')) st['f.collection'] = p.get('collection');
  const ow = OLD_WATCHED[p.get('watched')];
  if (ow) st['f.' + (ow[0] === 'unwatched' && sec.type === 'show' && !st.type ? 'unwatched_leaves' : ow[0])] = ow[1];
  if (!st.sort) st.sort = st.type === 'episode' || st.type === 'season' ? 'show' : 'title';
  const sortNow = parseSort(st.sort);
  const go = () => {
    const n = new URLSearchParams(); for (const [k, v] of Object.entries(st)) if (v) n.set(k, v);
    try { localStorage.setItem('libstate:' + id, n.toString()); } catch (e) { /* none */ }
    location.hash = `#/section/${id}?${n}`;
  };
  const advRules = (() => { try { return st.rules ? JSON.parse(st.rules) : []; } catch (e) { return []; } })();
  // The question to the server without paging and sorting (the letters use it as well).
  const filterQuery = () => {
    const q = new URLSearchParams();
    if (st.type) q.set('type', st.type);
    if (st.from) q.set('year>', st.from);
    if (st.to) q.set('year<', st.to);
    if (st.q) q.set('q', st.q);
    if (st.container) q.set('container', st.container);
    if (st.rules) q.set('rules', st.rules);
    if (st.rmatch) q.set('rmatch', st.rmatch);
    for (const [k, v] of Object.entries(st)) if (k.startsWith('f.') && v) q.set(k.slice(2), v);
    return q;
  };
  const sortParam = `${sortNow.field === 'show' ? 'episode_order' : sortNow.field}:${sortNow.desc ? 'desc' : 'asc'}`;
  const listUrl = (start, size = PAGE_SIZE) => { const q = filterQuery(); q.set('sort', sortParam); q.set('start', start); q.set('size', size); return `/library/sections/${id}/all?${q}`; };
  const first = (await GET(listUrl(0))).MediaContainer;
  const total = first.totalSize;
  const pick = (key, opts, title) => {
    const cur = opts.find(o => o[0] === st[key]) || opts[0];
    return dropBtn(cur[1], () => opts.map(([v, l]) => ({ label: l, checked: st[key] === v, run: () => { st[key] = v; if (key === 'type') { st.sort = ''; st.rules = ''; for (const k of Object.keys(st)) if (k.startsWith('f.')) delete st[k]; } go(); } })), { title, on: key !== 'sort' && !!st[key] });
  };
  const {filterBtn, chips, closePanel} = filterUI({ st, go, advRules, lastKey: 'libFilterField:' + sec.type + ':' + (st.type || ''),
    loadFilters: async () => (await GET(`/library/sections/${id}/filters?includeAdvanced=1${st.type ? '&type=' + st.type : ''}`)).MediaContainer.Filter });
  const text = (key, ph, w) => h('input', { placeholder: ph, value: st[key], style: `width:${w}px`, onchange: e => { st[key] = e.target.value.trim(); go(); } });
  // ----- sorting: the field and its direction
  const sortFields = (st.type === 'season' || st.type === 'episode' ? [['show', 'TV show']] : []).concat(SORT_FIELDS);
  const sortBtn = dropBtn(sortName(sortNow.field), () => sortFields.map(([f]) => ({ label: sortName(f), checked: f === sortNow.field, run: () => { st.sort = f + ':' + (SORT_ASC.has(f) ? 'asc' : 'desc'); go(); } })), { title: t('Sort') });
  const dirBtn = h('button', { class: 'dirbtn', title: sortNow.desc ? t('Descending') : t('Ascending'), 'aria-label': sortNow.desc ? t('Descending') : t('Ascending'),
    onclick: () => { st.sort = sortNow.field + ':' + (sortNow.desc ? 'asc' : 'desc'); go(); } }, sortNow.desc ? '↓' : '↑');
  // ----- the posters: the first page now, more while scrolling, a jump from the side letters
  const caption = captionFor(sortNow.field);
  const cardOpts = { episodeThumb: st.type === 'episode', caption };
  const holder = h('div');
  let grid = null;
  const pageOf = i => Math.floor(i / PAGE_SIZE);
  const have = new Set(), busy = new Set(), visible = new Set();
  // A page of titles takes the place of its empty cards. The first page also lays out the whole library
  // as empty cards, so the scroll bar and the letters are right from the start and nothing is loaded
  // that nobody scrolled to.
  const loadPage = async (p, firstPage) => {
    if (have.has(p) || busy.has(p)) return;
    busy.add(p);
    try {
      const r = firstPage || (await GET(listUrl(p * PAGE_SIZE))).MediaContainer;
      if (!grid) {
        grid = pickGrid(r.Metadata.slice(), cardOpts); holder.replaceChildren(grid);
        grid._reserve(total - r.Metadata.length);
        grid.querySelectorAll('.ph').forEach(el => more.observe(el));
      } else grid._put(p * PAGE_SIZE, r.Metadata);
      have.add(p);
    } catch (e) {
      // Tried again when the page is scrolled to the next time.
      toast(e.message);
      if (grid) for (let i = p * PAGE_SIZE; i < Math.min(total, (p + 1) * PAGE_SIZE); i++) { const el = grid.children[i]; if (el && el.classList.contains('ph')) more.observe(el); }
    } finally { busy.delete(p); }
  };
  // Empty cards that come near the screen load their page, unless they are only passed (dragging the scroll bar).
  const more = new IntersectionObserver(es => {
    for (const e of es) {
      if (!e.isIntersecting) { visible.delete(e.target); continue; }
      visible.add(e.target);
      setTimeout(() => {
        if (!visible.has(e.target) || !e.target.isConnected) return;
        more.unobserve(e.target); visible.delete(e.target);
        loadPage(pageOf(Number(e.target.dataset.i)));
      }, 150);
    }
  }, { rootMargin: '900px' });
  // A letter scrolls to its first poster at once; its page is fetched on the spot (nothing before it is).
  const jump = offset => {
    const card = grid && grid.children[Math.min(offset, grid.children.length - 1)];
    if (!card) return;
    const head = sticky.getBoundingClientRect().bottom;
    window.scrollTo({ top: window.scrollY + card.getBoundingClientRect().top - head - 12, behavior: 'auto' });
    loadPage(pageOf(Math.min(offset, total - 1)));
  };
  // The letters at the side (only when sorted by title).
  const letters = h('div', { class: 'letters' });
  const marks = []; // {btn, from, to}: the part of the list a letter stands for
  if (sortNow.field === 'title' && total > PAGE_SIZE / 2) {
    GET(`/library/sections/${id}/firstCharacter?${filterQuery()}`).then(r => {
      const dirs = r.MediaContainer.Directory; let sum = 0;
      letters.replaceChildren(...dirs.map(d => {
        // The server says where the letter's titles begin and end in the A-Z list (an older one only counts them).
        const from = d.start !== undefined ? d.start : sum, to = d.end !== undefined ? d.end : sum + d.size;
        sum += d.size;
        const lo = Math.max(0, sortNow.desc ? total - to : from), hi = sortNow.desc ? total - from : to;
        const btn = h('button', { title: `${d.key} · ${d.size}`, onclick: () => jump(lo) }, d.key);
        marks.push({ btn, from: lo, to: hi });
        return btn;
      }));
      showPos();
    }).catch(() => {});
  }
  const countEl = h('span', { class: 'count', title: t('{n} shown', { n: total }) }, itemCount(total));
  const sticky = h('div', { class: 'libhead' },
    h('h1', {}, sec.title, h('button', { class: 'more', title: t('More'), 'aria-label': t('More'), onclick: e => sectionMenu(sec, e.currentTarget) }, '⋯')),
    tabs,
    h('div', { class: 'bar-tools' },
      sec.type === 'show' ? pick('type', [['', t('Shows')], ['season', t('Seasons')], ['episode', t('Episodes')]]) : null,
      sec.type === 'music' ? pick('type', [['', t('Artists')], ['album', t('Albums')], ['track', t('Songs')]]) : null,
      filterBtn, sortBtn, dirBtn,
      text('from', t('Year from'), 95), text('to', t('Year to'), 85),
      pick('container', [['', t('Any file type')], ...['mkv', 'mp4', 'avi', 'mov', 'ts', 'm2ts', 'wmv', 'webm', 'mpg'].map(c => [c, c.toUpperCase()])]),
      text('q', t('Filter by title'), 150),
      countEl,
      h('span', { class: 'grow' }),
      sizeSlider(),
      S.me.admin ? h('button', { onclick: async () => { await api('POST', `/library/sections/${id}/refresh`); toast(t('Scanning {name}', { name: sec.title })); pollActivity(); } }, t('Scan now')) : null),
    chips);
  set(sticky, total ? holder : h('div', { class: 'empty' }, t('Nothing matches these filters.')), letters);
  if (total) {
    // Back from an item: the scroll position (the page is as long as before at once).
    const want = LIBPOS[myHash];
    await loadPage(0, first);
    if (want) window.scrollTo(0, want.y);
  }
  // Where you are: the first poster under the bar gives the number (in the counter) and the letter (lit in the side list).
  let raf = 0, lastPos = 0;
  const showPos = () => {
    raf = 0;
    if (location.hash !== myHash || !grid) return;
    let pos = lastPos;
    if (window.scrollY <= 40) pos = 0;
    else {
      // The card under the bar (empty cards count too); nothing found there: keep the last place.
      const r = grid.getBoundingClientRect(), y = sticky.getBoundingClientRect().bottom + 24;
      for (const x of [Math.max(0, r.left) + 24, r.left + r.width / 2]) {
        const el = document.elementFromPoint(x, y), c = el && el.closest && el.closest('.card[data-i]');
        if (c) { pos = Number(c.dataset.i); break; }
      }
    }
    lastPos = pos;
    countEl.textContent = t('{pos} / {total}', { pos: fmtNum(pos + 1), total: fmtNum(total) });
    for (const m of marks) m.btn.classList.toggle('on', pos >= m.from && pos < m.to);
  };
  const onPos = () => { if (!raf) raf = requestAnimationFrame(showPos); };
  window.addEventListener('scroll', onPos, { passive: true });
  let t0 = 0;
  const onScroll = () => {
    if (location.hash !== myHash) return;
    const now = Date.now(); if (now - t0 > 200) { t0 = now; LIBPOS[myHash] = { y: window.scrollY, n: total }; }
  };
  window.addEventListener('scroll', onScroll, { passive: true });
  window.addEventListener('hashchange', () => { more.disconnect(); closePanel(); window.removeEventListener('scroll', onScroll); window.removeEventListener('scroll', onPos); saveLibPos(); }, { once: true });
}

const itemCount = n => n === 1 ? t('1 item') : t('{n} items', { n });

// Collections, Playlists and Categories of one library.
async function sectionSubPage(sec, tab, tabs) {
  const head = h('h1', {}, sec.title);
  const tile = (d, onclick) => h('div', { class: 'card', onclick },
    h('div', { class: 'poster' }, d.thumb ? h('img', { src: withTok(d.thumb), loading: 'lazy', alt: '' }) : null, h('span', { class: 'badge' }, d.childCount)),
    h('div', { class: 't', title: d.title }, d.title), h('div', { class: 's' }, itemCount(d.childCount)));
  const open = (k, v) => () => (location.hash = `#/section/${sec.id}?${k}=${encodeURIComponent(v)}`);
  if (tab === 'collections') {
    const list = (await GET(`/library/sections/${sec.id}/collections`)).MediaContainer.Metadata;
    return set(head, tabs, list.length ? h('div', { class: 'grid' }, list.map(d => tile(d, open('f.collection', d.title))))
      : h('div', { class: 'empty' }, t('No collections yet. Film series are added when details are downloaded; add your own under Edit details → Collections.')));
  }
  if (tab === 'categories') {
    const list = (await GET(`/library/sections/${sec.id}/categories`)).MediaContainer.Metadata;
    return set(head, tabs, list.length ? h('div', { class: 'cats' }, list.map(d => h('div', { class: 'cat', onclick: open('f.genre', d.title) },
      d.art ? h('img', { src: withTok(d.art), loading: 'lazy', alt: '', onerror: e => { if (d.thumb && !e.target.dataset.f) { e.target.dataset.f = 1; e.target.src = withTok(d.thumb); } } }) : null,
      h('div', {}, d.title, h('small', {}, itemCount(d.childCount))))))
      : h('div', { class: 'empty' }, t('No categories yet. They come from the genres of the movies and shows.')));
  }
  const pls = meta(await GET(`/playlists?section=${sec.id}&sort=title`));
  set(head, tabs, pls.length ? h('div', { class: 'list' }, pls.map(pl => h('div', { class: 'li' },
    h('div', { class: 'grow' }, h('a', { href: '#/playlist/' + pl.id }, h('b', {}, plTitle(pl))),
      h('div', { class: 'dim' }, [pl.smart ? t('Smart playlist') : t('Playlist'), t('{n} items', { n: pl.leafCount ?? 0 }), pl.duration ? fmtDur(pl.duration) : null].filter(Boolean).join(' · '))))))
    : h('div', { class: 'empty' }, t('No playlists with anything from this library yet.')));
}
// The server's own playlists have English names; show them in the chosen language.
const plTitle = pl => pl.default ? t(pl.title) : pl.title;

// ---------- details
// Kinds of extras (trailers, featurettes …) as the scanner names them.
const ONLINE_VIDEO_TYPES = { Trailer: 'Trailer', Teaser: 'Teaser', Clip: 'Scene', Featurette: 'Featurette', 'Behind the Scenes': 'Behind the scenes' };
const EXTRA_TYPES = { trailer: 'Trailer', behindthescenes: 'Behind the scenes', deleted: 'Deleted scene', featurette: 'Featurette', interview: 'Interview', scene: 'Scene', short: 'Short', other: 'Extra' };

async function itemPage(id) {
  markNav('');
  const it = meta(await GET('/library/metadata/' + id))[0];
  const m = it.Media && it.Media[0];
  // Merged by hand (admin: "Merge"): several files of the same title; the choice is remembered per title.
  const vers = (it.Media || []).length > 1 ? it.Media : null;
  let vi = vers ? Math.min(Number(store.get('version:' + id, 0)) || 0, vers.length - 1) : 0;
  const playV = (at) => play(vi > 0 ? Object.assign({}, it, { id: vers[vi].id, ratingKey: String(vers[vi].id), Media: undefined }) : it, at);
  const versionName = v => [v.height ? (v.height >= 2000 ? '4K' : v.height + 'p') : '', v.edition, v.size ? fmtSize(v.size) : ''].filter(Boolean).join(' · ');
  const kids = ['show', 'season', 'artist', 'album'].includes(it.type) ? meta(await GET(`/library/metadata/${id}/children`)) : [];
  const refresh = () => itemPage(id);
  const watched = it.leafCount ? it.viewedLeafCount === it.leafCount : it.viewCount > 0;
  const crumbs = [];
  if (it.grandparentTitle) crumbs.push(h('a', { href: '#/item/' + it.grandparentRatingKey }, it.grandparentTitle), ' › ');
  if (it.parentTitle) crumbs.push(h('a', { href: '#/item/' + it.parentRatingKey }, seasonTitle(it.parentTitle)));
  const nextEp = async () => { const l = meta(await GET(`/library/metadata/${id}/allLeaves`)); const e = l.find(x => !x.viewCount) || l[0]; if (e) play(e, e.viewOffset ? e.viewOffset / 1000 : 0, l.slice(l.indexOf(e) + 1)); };
  // Online details (plot, cast, crew …) live in it.extra; seasons and episodes borrow the show's.
  const x = it.extra || {};
  const top = it.grandparentRatingKey || (it.type === 'season' ? it.parentRatingKey : null);
  const topX = top ? ((meta(await GET('/library/metadata/' + top))[0] || {}).extra || {}) : x;
  const cast = x.cast || (it.type !== 'movie' ? topX.cast : null) || [];
  const ids = x.ids || topX.ids || {};
  const crew = [[t('Directed by'), x.directors], [t('Written by'), x.writers], [t('Created by'), x.creators || (it.type !== 'show' ? topX.creators : null)]].filter(r => r[1] && r[1].length);
  const links = [ids.imdb ? ['IMDb', 'https://www.imdb.com/title/' + ids.imdb] : null, ids.tmdb ? ['TMDB', `https://www.themoviedb.org/${it.type === 'movie' ? 'movie' : 'tv'}/${ids.tmdb}`] : null,
    it.type === 'movie' && (ids.tmdb || ids.imdb) ? ['Letterboxd', ids.tmdb ? 'https://letterboxd.com/tmdb/' + ids.tmdb : 'https://letterboxd.com/imdb/' + ids.imdb] : null,
    ids.tvmaze ? ['TVmaze', 'https://www.tvmaze.com/shows/' + ids.tvmaze] : null, ids.wikipedia ? ['Wikipedia', ids.wikipedia] : null, ids.wikidata ? ['Wikidata', 'https://www.wikidata.org/wiki/' + ids.wikidata] : null].filter(Boolean);
  const matchable = it.type === 'movie' || it.type === 'show';
  const localTrailer = (it.Extras || []).find(e => e.extraType === 'trailer');
  const onlineVideos = matchable ? (x.trailers || []).filter(v => v && v.key) : [];
  // Rows: Other Editions, Related, More by the director, More with the first two actors.
  const chapters = m ? ((await GET(`/library/metadata/${id}/chapters`).catch(() => null)) || { MediaContainer: {} }).MediaContainer.Chapter || [] : [];
  const related = matchable && x.metadata_source ? ((await GET(`/library/metadata/${id}/related?count=20`)).MediaContainer.Hub || []) : [];
  const hero = h('img', { class: 'hero', src: withTok(`/library/metadata/${id}/art?v=${it.updatedAt || 0}`), alt: '', onerror: e => e.target.remove() });
  set(
    hero,
    h('div', { class: 'dim', style: 'margin-bottom:10px;position:relative' }, crumbs),
    h('div', { class: 'detail' },
      h('div', { class: 'poster', 'data-id': id }, h('img', { src: withTok(it.thumb), alt: '' }),
        isPlayable(it) && !it.missing ? h('button', { class: 'play', title: t('Play'), 'aria-label': t('Play'), onclick: () => play(it, (it.viewOffset || 0) / 1000) }, icon('play')) : null),
      h('div', { class: 'info' },
        h('h1', { style: 'margin-bottom:4px' }, it.type === 'episode' ? `${it.index}. ${it.title}` : it.type === 'season' ? seasonTitle(it.title) : it.clearLogo ? h('img', { src: withTok(it.clearLogo), alt: it.title, title: it.title, style: 'max-height:96px;max-width:100%;object-fit:contain;display:block', onerror: e => e.target.replaceWith(document.createTextNode(it.title)) }) : it.title,
          it.missing ? h('span', { class: 'tag', style: 'margin-left:10px;font-size:13px;vertical-align:middle;color:var(--red)', title: t('The file is gone from disk; the item is in the library’s trash') }, t('Unavailable')) : null),
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
          m && it.viewOffset ? h('button', { class: 'primary big', onclick: () => playV(it.viewOffset / 1000) }, t('▶ Resume from {time}', { time: fmtTime(it.viewOffset / 1000) })) : null,
          m && it.viewOffset ? iconBtn('restart', t('▶ Play from start'), () => playV(0)) : null,
          m && !it.viewOffset ? h('button', { class: 'primary big', onclick: () => playV(0) }, t('▶ Play')) : null,
          vers ? h('select', { title: t('Version'), 'aria-label': t('Version'), onchange: e => { vi = Number(e.target.value); store.set('version:' + id, vi); } },
            vers.map((v, i) => h('option', { value: i, selected: i === vi }, versionName(v)))) : null,
          // A trailer file next to the movie first, else the first online trailer.
          localTrailer ? h('button', { class: 'big', onclick: () => play(localTrailer, 0) }, t('▶ Trailer'))
            : onlineVideos.length ? h('button', { class: 'big', onclick: () => playOnlineVideo(onlineVideos[0]) }, t('▶ Trailer')) : null,
          !m && it.leafCount && (it.type === 'artist' || it.type === 'album') ? [
            h('button', { class: 'primary big', onclick: () => playMusic(id, false) }, t('▶ Play')),
            h('button', { class: 'big', title: t('Random order; the same show or artist does not come twice in a row while others are left'), onclick: () => playMusic(id, true) }, t('Shuffle'))] : null,
          !m && it.leafCount && it.type !== 'artist' && it.type !== 'album' ? h('button', { class: 'primary big', onclick: nextEp }, t('▶ Play next episode')) : null,
          iconBtn('check', watched ? t('Mark as unwatched') : t('Mark as watched'), async () => { await api('POST', `/:/${watched ? 'unscrobble' : 'scrobble'}?key=${it.id}`); refresh(); }, watched),
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
    kids.length && it.type !== 'artist' ? [h('h2', {}, it.type === 'show' ? t('Seasons') : it.type === 'album' ? t('Songs') : t('Episodes')),
      it.type === 'show' ? scroller('row', kids.map(k => posterCard(k))) :
        h('div', { class: 'list' }, kids.map((e, i) => h('div', { class: 'li' },
          it.type === 'album' ? null : h('img', { src: withTok(e.thumb), loading: 'lazy' }),
          h('div', { class: 'grow' }, h('a', { href: '#/item/' + e.id }, h('b', {}, e.index ? `${e.index}. ${e.title}` : e.title)), h('div', { class: 'dim' }, [fmtDur(e.duration), e.viewCount ? t('✓ watched') : '', e.viewOffset ? t('stopped at {time}', { time: fmtTime(e.viewOffset / 1000) }) : ''].filter(Boolean).join(' · ')),
            e.summary ? h('div', { class: 'ep-plot' }, e.summary) : null),
          h('button', { class: 'primary small', onclick: () => play(e, e.viewOffset ? e.viewOffset / 1000 : 0, kids.slice(i + 1)) }, '▶'))))] : null,
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
    related.map(hb => row(hb.title, hb.Metadata)));
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
  tmdb: () => '<svg viewBox="0 0 70 32" aria-hidden="true"><defs><linearGradient id="tmdb-mark" x1="0" x2="1"><stop offset="0" stop-color="#90cea1"/><stop offset=".56" stop-color="#3cbec9"/><stop offset="1" stop-color="#00b3e5"/></linearGradient></defs><rect width="70" height="32" rx="16" fill="url(#tmdb-mark)"/><text x="35" y="22" text-anchor="middle" font-family="Arial,Helvetica,sans-serif" font-weight="900" font-size="16" fill="#0d253f">TMDB</text></svg>',
};

// Scores from review sites with their marks. Values are as the source gives them.
function scoreBadges(r) {
  if (!r) return null;
  const names = [['imdb', 'IMDb'], ['rotten_tomatoes', 'Rotten Tomatoes'], ['rotten_tomatoes_audience', 'Rotten Tomatoes audience'], ['metacritic', 'Metacritic'], ['tmdb', 'TMDB']];
  const shown = names.filter(([k]) => r[k] && r[k].score);
  if (!shown.length) return null;
  return h('div', { class: 'scores' }, shown.map(([k, n]) => {
    const s = r[k];
    const count = s.count ?? s.votes;
    const many = count ? t(s.count_label === 'reviews' ? '{n} reviews' : '{n} votes', { n: Number(count).toLocaleString(I18N.lang) }) : '';
    const mark = h('span', { class: 'mark' });
    const num = parseFloat(String(s.score)) || 0;
    mark.innerHTML = SCORE_MARKS[k](k === 'rotten_tomatoes_audience' ? (String(s.score).includes('/5') ? num * 20 : String(s.score).includes('/10') ? num * 10 : num) : num);
    const kids = [mark, h('span', { class: 'num' }, h('b', {}, s.score), h('span', {}, many || n))];
    const title = `${n}: ${s.score}${many ? ' · ' + many : ''}${s.note ? ' — ' + s.note : ''}`;
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
    : p.phase === 'finishing' ? t('Scanning {name}: finishing', { name: p.name }) : t('Scanning {name}: looking through the folders', { name: p.name });
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

// Where the end credits start: decides when the video counts as watched.
async function creditsDialog(it) {
  const r = await GET(`/library/metadata/${it.id}/markers`);
  const parse = v => { const p = v.trim().split(':').map(Number); if (!v.trim() || p.some(isNaN)) return null; return p.reduce((a, b) => a * 60 + b, 0) * 1000; };
  const inp = h('input', { value: r.credits.length ? fmtTime(r.credits[0] / 1000) : '', placeholder: t('e.g. 1:52:30'), style: 'width:140px' });
  const from = { manual: t('set by hand'), chapters: t('from a chapter named Credits in the file'), picture: t('found in the picture (dark screen with text near the end)'), none: t('none found (no Credits chapter, and the end of the video doesn’t look like credits)'), pending: t('not looked for yet (detection runs in the background)') }[r.source] + (r.errors ? t('; detection failed {n} time(s)', { n: r.errors }) : '');
  const save = async credits => { try { await api('PUT', `/library/metadata/${it.id}/markers`, { credits }); toast(t('Saved')); box.remove(); } catch (e) { toast(e.message, 5000); } };
  const box = modal(t('End credits'), [
    h('p', { class: 'dim' }, t('Credits marker: {from}. This video counts as watched at {at} of {length}.', { from, at: fmtTime(r.watchedAt / 1000), length: fmtTime(r.duration / 1000) })),
    formRow(t('Credits start at (hours:minutes:seconds)'), inp),
    h('div', { class: 'actions' },
      h('button', { class: 'primary', onclick: () => { const ms = parse(inp.value); if (ms === null) return toast(t('Enter a time like 1:52:30')); save([ms]); } }, t('Save')),
      r.source === 'manual' ? h('button', { onclick: () => save(null) }, t('Use the file’s chapters')) : null,
      h('button', { title: t('Read the chapters of this file again and look for the end credits'), onclick: async () => {
        try { await api('POST', `/library/metadata/${it.id}/credits`); toast(t('Detecting credits…')); box.remove(); pollActivity(); setTimeout(() => creditsDialog(it), 2500); } catch (e) { toast(e.message, 6000); }
      } }, t('Detect credits again')),
      h('button', { onclick: () => box.remove() }, t('Cancel')))]);
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
        results.replaceChildren(h('div', { class: 'dim' }, t('Downloading details for {title}…', { title: c.title })));
        try { const a = await api('PUT', `/library/metadata/${it.id}/match`, { source: c.source, id: c.id }); toast(t(a.message), 4000); box.remove(); if (done) done(); else itemPage(it.id); }
        catch (e) { results.replaceChildren(h('div', { class: 'err' }, e.message)); }
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
      await api('PUT', '/library/metadata/' + it.id, body); box.remove(); saved ? saved() : itemPage(it.id);
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

// ---------- player
// Own controls over the <video>: one seek bar for the whole video (also while converting, where the
// stream starts at the seek point, and for movies split over files), buffer, credits mark and a
// picture preview; volume, subtitles, audio, quality, speed, picture-in-picture, full screen,
// "Skip credits", the next episode with a countdown and keyboard shortcuts (? lists them).
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
  'timeFormat', 'cardSize', 'dlQuality', 'navMore', 'quality', 'directPlay', 'keepSmall', 'levelVolume', 'themeMusic', 'rememberTab', 'autoNext', 'timeLeft', 'speed'];
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
  // What the player read when this file loaded.
  P.quality = store.get('quality', 'original'); P.speed = Number(store.get('speed', 1)) || 1;
  P.autoNext = store.get('autoNext', '1') === '1'; P.remaining = store.get('timeLeft', '0') === '1';
  cardSize();
}
window.saveLangPref = code => (S.me ? api('POST', '/api/me/prefs', { lang: code }).catch(() => {}) : null);
const P = { it: null, offset: 0, method: 'direct', timer: null, queue: [], quality: store.get('quality', 'original'), audio: '', speed: Number(store.get('speed', 1)) || 1,
  sub: -1, subSize: store.get('subSize', '100'), autoNext: store.get('autoNext', '1') === '1', remaining: store.get('timeLeft', '0') === '1',
  next: null, credits: [], intro: null, pending: null, pendingTimer: null, countdown: null, menu: null };
const SPEEDS = [0.5, 0.75, 1, 1.25, 1.5, 1.75, 2];
const QUALITIES = [['original', t('Original quality')], ['1080p-20', '1080p 20 Mbps'], ['1080p-12', '1080p 12 Mbps'], ['1080p-8', '1080p 8 Mbps'], ['720p-4', '720p 4 Mbps'], ['720p-3', '720p 3 Mbps'], ['720p-2', '720p 2 Mbps'], ['480p-1.5', '480p ' + fmtNum(1.5) + ' Mbps'], ['320p-0.7', '320p ' + fmtNum(0.7) + ' Mbps']];
const SUB_SIZES = [['75', t('Small')], ['100', t('Normal')], ['130', t('Large')], ['170', t('Very large')]];
const video = $('#video');
const player = $('#player');
const bar = $('#pbar');
video.volume = Math.min(1, Math.max(0, Number(store.get('volume', 1))));
video.muted = store.get('muted', '0') === '1';

// Icons (Material-style paths, drawn in the text colour).
const ICONS = {
  play: 'M8 5v14l11-7z', pause: 'M6 19h4V5H6v14zm8-14v14h4V5h-4z',
  back: 'M11.99 5V1l-5 5 5 5V7c3.31 0 6 2.69 6 6s-2.69 6-6 6-6-2.69-6-6h-2c0 4.42 3.58 8 8 8s8-3.58 8-8-3.58-8-8-8z',
  fwd: 'M18 13c0 3.31-2.69 6-6 6s-6-2.69-6-6 2.69-6 6-6v4l5-5-5-5v4c-4.42 0-8 3.58-8 8s3.58 8 8 8 8-3.58 8-8h-2z',
  next: 'M6 18l8.5-6L6 6v12zM16 6v12h2V6h-2z',
  vol: 'M3 9v6h4l5 5V4L7 9H3zm13.5 3A4.5 4.5 0 0 0 14 7.97v8.05c1.48-.73 2.5-2.25 2.5-4.02zM14 3.23v2.06c2.89.86 5 3.54 5 6.71s-2.11 5.85-5 6.71v2.06c4.01-.91 7-4.49 7-8.77s-2.99-7.86-7-8.77z',
  low: 'M7 9v6h4l5 5V4l-5 5H7zm11.5 3A4.5 4.5 0 0 0 16 7.97v8.05c1.48-.73 2.5-2.25 2.5-4.02z',
  mute: 'M16.5 12A4.5 4.5 0 0 0 14 7.97v2.21l2.45 2.45c.03-.2.05-.41.05-.63zm2.5 0c0 .94-.2 1.82-.54 2.64l1.51 1.51A8.8 8.8 0 0 0 21 12c0-4.28-2.99-7.86-7-8.77v2.06c2.89.86 5 3.54 5 6.71zM4.27 3 3 4.27 7.73 9H3v6h4l5 5v-6.73l4.25 4.25c-.67.52-1.42.93-2.25 1.18v2.06a8.99 8.99 0 0 0 3.69-1.81L19.73 21 21 19.73l-9-9L4.27 3zM12 4 9.91 6.09 12 8.18V4z',
  cc: 'M19 4H5a2 2 0 0 0-2 2v12a2 2 0 0 0 2 2h14c1.1 0 2-.9 2-2V6a2 2 0 0 0-2-2zm-8 7H9.5v-.5h-2v3h2V13H11v1c0 .55-.45 1-1 1H7c-.55 0-1-.45-1-1v-4c0-.55.45-1 1-1h3c.55 0 1 .45 1 1v1zm7 0h-1.5v-.5h-2v3h2V13H18v1c0 .55-.45 1-1 1h-3c-.55 0-1-.45-1-1v-4c0-.55.45-1 1-1h3c.55 0 1 .45 1 1v1z',
  gear: 'M19.14 12.94a7.07 7.07 0 0 0 .06-.94c0-.32-.02-.64-.07-.94l2.03-1.58a.49.49 0 0 0 .12-.61l-1.92-3.32a.49.49 0 0 0-.59-.22l-2.39.96a7.03 7.03 0 0 0-1.62-.94l-.36-2.54a.48.48 0 0 0-.48-.41h-3.84c-.24 0-.43.17-.47.41l-.36 2.54c-.59.24-1.13.57-1.62.94l-2.39-.96a.49.49 0 0 0-.59.22L2.74 8.87a.47.47 0 0 0 .12.61l2.03 1.58c-.05.3-.09.63-.09.94s.02.64.07.94l-2.03 1.58a.49.49 0 0 0-.12.61l1.92 3.32c.12.22.37.29.59.22l2.39-.96c.5.38 1.03.7 1.62.94l.36 2.54c.05.24.24.41.48.41h3.84c.24 0 .44-.17.47-.41l.36-2.54c.59-.24 1.13-.56 1.62-.94l2.39.96c.22.08.47 0 .59-.22l1.92-3.32a.47.47 0 0 0-.12-.61l-2.01-1.58zM12 15.6A3.6 3.6 0 1 1 12 8.4a3.6 3.6 0 0 1 0 7.2z',
  pip: 'M19 11h-8v6h8v-6zm4 8V4.98C23 3.88 22.1 3 21 3H3c-1.1 0-2 .88-2 1.98V19c0 1.1.9 2 2 2h18c1.1 0 2-.9 2-2zm-2 .02H3V4.97h18v14.05z',
  full: 'M7 14H5v5h5v-2H7v-3zm-2-4h2V7h3V5H5v5zm12 7h-3v2h5v-5h-2v3zM14 5v2h3v3h2V5h-5z',
  exitfull: 'M5 16h3v3h2v-5H5v2zm3-8H5v2h5V5H8v3zm6 11h2v-3h3v-2h-5v5zm2-11V5h-2v5h5V8h-3z',
  stop: 'M6 6h12v12H6z',
  close: 'M20 11H7.83l5.59-5.59L12 4l-8 8 8 8 1.41-1.41L7.83 13H20v-2z',
  trash: 'M6 19c0 1.1.9 2 2 2h8c1.1 0 2-.9 2-2V7H6v12zM19 4h-3.5l-1-1h-5l-1 1H5v2h14V4z',
  check: 'M9 16.17 4.83 12l-1.42 1.41L9 19 21 7l-1.41-1.41z',
  heart: 'M12 21.35l-1.45-1.32C5.4 15.36 2 12.28 2 8.5 2 5.42 4.42 3 7.5 3c1.74 0 3.41.81 4.5 2.09C13.09 3.81 14.76 3 16.5 3 19.58 3 22 5.42 22 8.5c0 3.78-3.4 6.86-8.55 11.54L12 21.35z',
  heartOff: 'M16.5 3c-1.74 0-3.41.81-4.5 2.09C10.91 3.81 9.24 3 7.5 3 4.42 3 2 5.42 2 8.5c0 3.78 3.4 6.86 8.55 11.54L12 21.35l1.45-1.32C18.6 15.36 22 12.28 22 8.5 22 5.42 19.58 3 16.5 3zm-4.4 15.55-.1.1-.1-.1C7.14 14.24 4 11.39 4 8.5 4 6.5 5.5 5 7.5 5c1.54 0 3.04.99 3.57 2.36h1.87C13.46 5.99 14.96 5 16.5 5c2 0 3.5 1.5 3.5 3.5 0 2.89-3.14 5.74-7.9 10.05z',
  lyrics: 'M20 2H4c-1.1 0-2 .9-2 2v18l4-4h14c1.1 0 2-.9 2-2V4c0-1.1-.9-2-2-2zm-3 12H7v-2h10v2zm0-3H7V9h10v2zm0-3H7V6h10v2z',
  mark: 'M17 3H7c-1.1 0-2 .9-2 2v16l7-3 7 3V5c0-1.1-.9-2-2-2z',
  markOff: 'M17 3H7c-1.1 0-2 .9-2 2v16l7-3 7 3V5c0-1.1-.9-2-2-2zm0 15-5-2.18L7 18V5h10v13z',
  restart: 'M12 5V1L7 6l5 5V7c3.31 0 6 2.69 6 6s-2.69 6-6 6-6-2.69-6-6H4c0 4.42 3.58 8 8 8s8-3.58 8-8-3.58-8-8-8z',
  down: 'M7 10l5 5 5-5z',
  filter: 'M10 18h4v-2h-4v2zM3 6v2h18V6H3zm3 7h12v-2H6v2z',
  edit: 'M3 17.25V21h3.75L17.81 9.94l-3.75-3.75L3 17.25zM20.71 7.04a1 1 0 0 0 0-1.41l-2.34-2.34a1 1 0 0 0-1.41 0l-1.83 1.83 3.75 3.75 1.83-1.83z',
  more: 'M6 10c-1.1 0-2 .9-2 2s.9 2 2 2 2-.9 2-2-.9-2-2-2zm12 0c-1.1 0-2 .9-2 2s.9 2 2 2 2-.9 2-2-.9-2-2-2zm-6 0c-1.1 0-2 .9-2 2s.9 2 2 2 2-.9 2-2-.9-2-2-2z',
};
// A round button with just an icon; the name is in the tooltip. on (true/false) = a toggle and whether it is set.
const iconBtn = (name, label, onclick, on) => h('button', { class: 'ibtn' + (on ? ' on' : ''), title: label, 'aria-label': label, 'aria-pressed': on === undefined ? null : String(on), onclick }, icon(name));
const icon = name => { const s = document.createElementNS('http://www.w3.org/2000/svg', 'svg'); s.setAttribute('viewBox', '0 0 24 24'); s.innerHTML = `<path d="${ICONS[name]}"/>`; return s; };
function btn(id, name, title, label) {
  const b = $('#' + id); b.replaceChildren(icon(name)); if (label) b.append(label);
  b.title = title; b.setAttribute('aria-label', title);
}
btn('pclose', 'close', t('Close (Esc)'));
btn('pback', 'back', t('Back 10 seconds (←)'));
btn('pfwd', 'fwd', t('Forward 30 seconds (→)'));
btn('pnextbtn', 'next', t('Next (N)'));
btn('psubs', 'cc', t('Subtitles (C)'));
btn('plyricsbtn', 'lyrics', t('Lyrics (Y)'));
btn('psettings', 'gear', t('Settings'));
btn('ppip', 'pip', t('Picture in picture (P)'));
$('#pvolume').setAttribute('aria-label', t('Volume'));
$('#ptime').title = t('Click: show time left');
$('#ppip').hidden = !document.pictureInPictureEnabled;

const duration = () => (P.it && P.it.duration ? P.it.duration / 1000 : 0) || (P.method === 'direct' && !split() && isFinite(video.duration) ? video.duration : 0);
const position = () => (P.base || 0) + P.offset + (video.currentTime || 0);
// What the bar shows: a seek waiting to happen, else where the video is.
const shown = () => P.pending !== null ? P.pending : position();

// together = following a "Watch together" room (not told to the others again).
async function play(it, startAt, queue, together) {
  if (typeof stopTheme === 'function') stopTheme();
  P.queue = queue || [];
  if (!it.Media || !it.Media[0].streams) it = meta(await GET('/library/metadata/' + it.id))[0];
  P.it = it;
  player.style.display = 'flex';
  hideNext(); closePlayerMenu();
  $('#pkeys').style.display = 'none';
  const ep = it.type === 'episode', song = it.type === 'track';
  $('#ptitle').textContent = ep ? it.grandparentTitle : it.title;
  P.subLine = ep ? `${t('S{n}', { n: it.parentIndex })}${t('E{n}', { n: it.index })} · ${it.title}` : song ? [it.grandparentTitle, it.parentTitle].filter(Boolean).join(' · ') : (it.year ? String(it.year) : '');
  // A song has no picture: the cover stays on the screen.
  video.poster = song ? withTok(it.thumb) : '';
  const auds = ((it.Media && it.Media[0].streams) || []).filter(s => s.kind === 'audio');
  P.audios = auds;
  P.audio = '';
  P.burn = null;
  P.subs = (it.Media && it.Media[0].subtitles) || [];
  // Preferred audio language (Settings → Web), when the file has it and it isn't the first track.
  const prefAudio = store.get('audioLang', '');
  const aPick = prefAudio ? auds.findIndex(a => lang2(a.language) === prefAudio) : -1;
  if (aPick > 0) P.audio = auds[aPick].index;
  // The subtitle language picked last time comes back on its own, else the subtitle mode decides.
  const wantLang = store.get('subLang', '');
  P.sub = wantLang ? P.subs.findIndex(s => (s.language || s.file || '') === wantLang) : -1;
  const subMode = store.get('subMode', 'manual'), prefSub = store.get('subPrefLang', '');
  if (P.sub < 0 && prefSub) {
    const playing = auds[aPick > 0 ? aPick : 0];
    const foreign = !playing || !playing.language || lang2(playing.language) !== prefSub;
    if (subMode !== 'manual' && (subMode === 'always' || foreign)) P.sub = pickSub(P.subs, prefSub, store.get('subSdh', 'any'), false);
    // Forced subtitles (only the foreign lines) come on by themselves when the audio is in the preferred language.
    else if (!foreign && store.get('subForced', '1') === '1') P.sub = pickSub(P.subs, prefSub, 'any', true);
  }
  P.lastSub = P.sub >= 0 ? P.sub : undefined;
  // A picture-based subtitle (Blu-ray, DVD) can't be shown as text: it is burned into the video.
  if (P.sub >= 0 && isPictureSub(P.subs[P.sub])) { if (store.get('burnSubs', 'auto') === 'auto') { P.burn = P.sub; } P.sub = -1; }
  P.credits = (it.credits || []).map(ms => ms / 1000);
  P.chapters = [];
  if (it.Media && it.Media[0]) GET(`/library/metadata/${it.id}/chapters`).then(r => { if (P.it !== it) return; P.chapters = (r.MediaContainer.Chapter || []).map(c => ({ at: c.start / 1000, title: c.title.replace(/^Chapter (\d+)$/, (_, n) => t('Chapter {n}', { n })) })); drawMarks(); }).catch(() => {});
  P.intro = it.intro && it.intro.end > it.intro.start ? [it.intro.start / 1000, it.intro.end / 1000] : null;
  // A movie split over files: its parts play one after the other.
  P.parts = ((it.Media && it.Media[0].Part) || []).map(x => (x.duration || 0) / 1000);
  P.part = 0; P.base = 0; P.pending = null;
  P.next = null; P.nextFor = it.id;
  findNext(it);
  drawMarks();
  mediaSession(it);
  loadLyrics(it);
  // Settings -> Extras -> pre-roll: a short video before a movie that starts from the beginning.
  if (it.type === 'movie' && !(startAt > 0) && !together) await playPreroll(it);
  if (P.it !== it) return;
  await load(startAt || 0);
  if (TG.code && !together) { TG.quiet = 0; tgSend({ item: it.id, position: Math.round((startAt || 0) * 1000) }); }
  wake();
}
// ---- lyrics of a song: lines that follow the music (.lrc) or just the text
function loadLyrics(it) {
  P.lyrics = null; P.lyricLine = -1;
  const box = $('#plyrics');
  box.style.display = 'none'; box.replaceChildren(); $('#plyricsbtn').hidden = true;
  if (it.type !== 'track') return;
  GET(`/library/metadata/${it.id}/lyrics`).then(l => {
    if (P.it !== it || !l.lines || !l.lines.length) return;
    P.lyrics = l;
    box.classList.toggle('sync', !!l.synced);
    box.replaceChildren(...l.lines.map(x => h('p', l.synced && x.t !== undefined ? { onclick: () => seekTo(x.t / 1000) } : {}, x.text || '\u00a0')));
    $('#plyricsbtn').hidden = false;
    showLyrics(store.get('lyricsOn', '1') === '1');
  }).catch(() => { /* a song without words */ });
}
function showLyrics(on) {
  const box = $('#plyrics');
  box.style.display = on && P.lyrics ? 'block' : 'none';
  $('#plyricsbtn').classList.toggle('on', !!(on && P.lyrics));
  P.lyricLine = -2;
  drawLyrics();
}
function toggleLyrics() {
  if (!P.lyrics) { toast(t('No lyrics for this song')); return; }
  const on = $('#plyrics').style.display === 'none';
  store.set('lyricsOn', on ? '1' : '0');
  showLyrics(on);
}
// Marks the line that is sung now and keeps it in the middle.
function drawLyrics() {
  const box = $('#plyrics');
  if (!P.lyrics || !P.lyrics.synced || box.style.display === 'none') return;
  const now = position() * 1000;
  let cur = -1;
  P.lyrics.lines.forEach((x, i) => { if (x.t <= now) cur = i; });
  if (cur === P.lyricLine) return;
  const ps = box.children;
  if (P.lyricLine >= 0 && ps[P.lyricLine]) ps[P.lyricLine].classList.remove('now');
  P.lyricLine = cur;
  if (cur >= 0 && ps[cur]) { ps[cur].classList.add('now'); box.scrollTop = ps[cur].offsetTop - box.clientHeight / 2 + ps[cur].clientHeight / 2; }
}
$('#plyricsbtn').addEventListener('click', e => { e.stopPropagation(); toggleLyrics(); });
// Plays the pre-roll video (if one is set) until it ends, fails or "Skip" is pressed.
async function playPreroll(it) {
  let urls = [];
  try { urls = (await GET('/api/preroll')).urls || []; } catch (e) { return; }
  for (const url of urls) { if (P.it !== it || player.style.display === 'none') return; if (!(await playPrerollVideo(it, url, urls.length > 1))) return; }
}
// One pre-roll video; false when "Skip" was pressed on the last one or the player was closed.
async function playPrerollVideo(it, url, several) {
  return new Promise(done => {
    const skip = h('button', { class: 'primary', style: 'position:absolute;right:24px;bottom:90px;z-index:5' }, several ? t('Skip all') : t('Skip'));
    const next = several ? h('button', { style: 'position:absolute;right:24px;bottom:134px;z-index:5' }, t('Next video')) : null;
    let finished = false, all = false;
    const end = () => {
      if (finished) return; finished = true;
      video.removeEventListener('ended', end); video.removeEventListener('error', end);
      skip.remove(); if (next) next.remove(); done(!all && P.it === it && player.style.display !== 'none');
    };
    if (next) { next.onclick = end; player.append(next); }
    skip.onclick = () => { all = true; end(); };
    player.append(skip);
    video.addEventListener('ended', end); video.addEventListener('error', end);
    video.querySelectorAll('track').forEach(tr => tr.remove());
    video.src = url;
    video.play().catch(end);
    // The player was closed meanwhile.
    const watch = setInterval(() => { if (finished) return clearInterval(watch); if (P.it !== it || player.style.display === 'none') { clearInterval(watch); end(); } }, 500);
  });
}
// What plays after this one: the player's own queue, then "Add to queue", then the show's next episode.
async function findNext(it) {
  $('#pnextbtn').hidden = true;
  let next = P.queue[0] || Q.list[0] || null;
  if (!next && it.type === 'track' && it.parentRatingKey) {
    try {
      const all = meta(await GET(`/library/metadata/${it.parentRatingKey}/allLeaves`));
      const i = all.findIndex(x => String(x.id) === String(it.id));
      if (i >= 0 && all[i + 1]) next = all[i + 1];
    } catch (e) { /* the last song then */ }
  }
  if (!next && it.type === 'episode' && it.grandparentRatingKey) {
    try {
      const all = meta(await GET(`/library/metadata/${it.grandparentRatingKey}/allLeaves`));
      const i = all.findIndex(x => String(x.id) === String(it.id));
      if (i >= 0 && all[i + 1]) next = all[i + 1];
    } catch (e) { /* no next episode then */ }
  }
  // A film of a series: the next film of it that is not watched yet is offered at the end (not started by itself).
  P.suggest = false;
  if (!next && it.type === 'movie' && it.librarySectionID) {
    try {
      const hubs = (await GET(`/library/metadata/${it.id}/related?count=50`)).MediaContainer.Hub || [];
      const list = ((hubs.find(x => x.hubIdentifier === 'movie.collection') || {}).Metadata || []).filter(x => !(x.viewCount > 0) && (x.year || 9999) >= (it.year || 0));
      if (list.length) { next = list[0]; P.suggest = true; }
    } catch (e) { /* no suggestion then */ }
  }
  if (P.nextFor !== it.id) return; // another video started meanwhile
  P.next = next;
  $('#pnextbtn').hidden = !next;
  if (next) $('#pnextbtn').title = t('Next (N)') + ': ' + nextName(next);
}
const RELEASES = ['album', 'ep', 'single', 'live', 'compilation', 'soundtrack'];
const releaseName = (r, plural) => ({ album: plural ? t('Albums') : t('Album'), ep: plural ? t('EPs') : t('EP'), single: plural ? t('Singles') : t('Single'),
  live: plural ? t('Live albums') : t('Live'), compilation: plural ? t('Compilations') : t('Compilation'), soundtrack: plural ? t('Soundtracks') : t('Soundtrack') }[r] || r);
// Plays all songs of an artist or album, one after the other (shuffle: not the same artist twice in a row).
async function playMusic(id, shuffle) {
  let songs = meta(await GET(`/library/metadata/${id}/allLeaves`));
  if (!songs.length) return;
  if (shuffle) songs = smartShuffle(songs);
  play(songs[0], 0, songs.slice(1));
}
const nextName = n => n.type === 'episode' ? `${t('S{n}', { n: n.parentIndex })}${t('E{n}', { n: n.index })} · ${n.title}` : n.title;
function playNext() {
  hideNext();
  let next = P.queue.shift();
  if (!next && Q.list.length) { next = Q.list.shift(); saveQueue(); }
  if (!next) next = P.next;
  if (next) play(next, 0, P.queue); else closePlayer(false);
}
// Jump to a place in the whole video.
async function seekTo(s) {
  s = Math.max(0, Math.min(s, Math.max(0, duration() - 0.5)));
  P.pending = null;
  if (P.method === 'direct' && !split()) video.currentTime = s; else await load(s);
  hideNext();
  drawBar();
}
// Arrow keys and the ±buttons: while converting, several presses become one jump.
function nudge(delta) {
  const target = Math.max(0, Math.min(shown() + delta, duration() || Infinity));
  flash(delta < 0 ? 'back' : 'fwd', (delta > 0 ? '+' : '') + delta + ' s');
  if (P.method === 'direct' && !split()) return seekTo(target);
  P.pending = target;
  drawBar();
  clearTimeout(P.pendingTimer);
  P.pendingTimer = setTimeout(async () => { const to = P.pending; if (to === null) return; await seekTo(to); tgSend(); }, 450);
}
// Where part n starts in the whole movie (seconds).
const partStart = n => P.parts.slice(0, n).reduce((a, b) => a + b, 0);
// (Only when every part's length is known; otherwise the parts can't be told apart.)
const split = () => P.parts.length > 1 && P.parts.every(d => d > 0);

// Settings -> Web -> level the volume: videos the server has measured are brought to about -23 LUFS.
const LEVEL = { ctx: null, gain: null };
function levelVolume(it) {
  try {
    const on = store.get('levelVolume', '0') === '1' && typeof it.loudness === 'number';
    if (!on && !LEVEL.gain) return;
    if (!LEVEL.gain) {
      LEVEL.ctx = new (window.AudioContext || window.webkitAudioContext)();
      LEVEL.gain = LEVEL.ctx.createGain();
      LEVEL.ctx.createMediaElementSource(video).connect(LEVEL.gain);
      LEVEL.gain.connect(LEVEL.ctx.destination);
    }
    LEVEL.gain.gain.value = on ? Math.min(2, Math.max(0.3, Math.pow(10, (-23 - it.loudness) / 20))) : 1;
    LEVEL.ctx.resume();
  } catch (e) { /* the video just plays at its own volume */ }
}
async function load(at) {
  const it = P.it;
  levelVolume(it);
  let q = P.quality;
  // Settings → Web → quality: a video that is not bigger than the chosen quality plays as it is.
  const want = Number((/^(\d+)p/.exec(q) || [])[1]) || 0, have = Number(it.Media && it.Media[0] && it.Media[0].height) || 0;
  if (want && have && have <= want && store.get('keepSmall', '1') === '1') q = 'original';
  // Which part the position is in; `at` becomes the time inside that part.
  P.part = 0;
  if (split()) while (P.part < P.parts.length - 1 && at >= partStart(P.part + 1)) P.part++;
  P.base = split() ? partStart(P.part) : 0;
  at = Math.max(0, at - P.base);
  const extra = (CLIENT_CODECS ? `&videoCodecs=${CLIENT_CODECS}` : '') + (P.audio ? `&audio=${P.audio}` : '') + (P.part ? `&part=${P.part}` : '') + (P.burn !== null && P.burn !== undefined ? `&subtitle=${P.burn}` : '');
  const d = await GET(`/library/metadata/${it.id}/decision?quality=${q}${extra}`);
  if (P.it !== it) return;
  P.method = P.audio && d.method === 'direct' ? 'remux' : d.method;
  if (!d.allowed) toast(t('Converting is switched off for your account, so this file may not play here.'), 5000);
  video.querySelectorAll('track').forEach(tr => tr.remove());
  spin(true);
  const noDirect = store.get('directPlay', '1') === '0' && P.method === 'direct' && d.allowed;
  if (noDirect) P.method = 'transcode';
  if (P.method === 'direct') {
    P.offset = 0;
    video.src = withTok(`/library/parts/${it.id}/file${P.part ? '?part=' + P.part : ''}`);
    video.addEventListener('loadedmetadata', () => { video.currentTime = at; }, { once: true });
  } else {
    P.offset = at;
    video.src = withTok(`/video/transcode/${it.id}/stream.mp4?quality=${q}&offset=${at.toFixed(1)}${extra}${noDirect ? '&transcode=1' : ''}`);
  }
  addSubtitles();
  $('#pmethod').textContent = { direct: ' · ' + t('direct play'), remux: ' · ' + t('repackaged'), transcode: ' · ' + t('converting') }[P.method] || '';
  // A new source starts at the default rate: keep the chosen speed.
  video.defaultPlaybackRate = video.playbackRate = P.speed;
  // (The browser may refuse to start by itself; then the play button is there.)
  video.play().catch(() => { spin(false); drawButtons(); });
  clearInterval(P.timer);
  P.timer = setInterval(() => { if (!video.paused) report('playing'); }, 10000);
  drawBar();
}
// Subtitles belong to the first file (a later part would show them at the wrong time). A converted
// stream starts at the seek point, so its times are moved by that much.
function subTrack(s, i, shift, fresh) {
  const el = h('track', { kind: 'subtitles', src: withTok(s.key + (fresh ? '?r=' + Date.now() : '')), label: subName(s, i), srclang: (s.language || 'und').slice(0, 2) });
  el.addEventListener('load', () => {
    if (shift > 0 && el.track.cues) for (const c of Array.from(el.track.cues)) { c.startTime = Math.max(0, c.startTime - shift); c.endTime = Math.max(0, c.endTime - shift); }
  }, { once: true });
  el.addEventListener('error', () => { if (i === P.sub) toast(t('This subtitle can’t be shown here.'), 4000); }, { once: true });
  return el;
}
function addSubtitles() {
  if (P.part) return;
  P.subs.forEach((s, i) => video.append(subTrack(s, i, P.offset)));
  showSub();
}
// Fetches one subtitle track again (after its timing changed), in the same place.
function reloadSub(i) {
  const old = video.querySelectorAll('track')[i];
  if (!old || !P.subs[i]) return;
  old.replaceWith(subTrack(P.subs[i], i, P.offset, true));
  showSub();
}
// Subtitle timing (owner): listen to the sound and move the lines onto the speech, or nudge by hand.
async function syncSub() {
  const s = P.subs[P.sub]; if (!s) return;
  closePlayerMenu();
  toast(t('Listening to the sound … this takes a moment'), 60000);
  try {
    const r = await api('POST', s.key + '/sync');
    if (!r.applied) { toast(t(r.message), 6000); return; }
    s.offsetMs = r.offsetMs;
    reloadSub(P.sub);
    toast(t('Subtitles moved by {s} s', { s: fmtNum(r.offsetMs / 1000, 1) }) + (Math.abs(r.scale - 1) > 1e-6 ? ' · ' + t('and stretched for another frame rate') : ''), 5000);
  } catch (e) { toast(e.message, 6000); }
}
async function nudgeSub(ms) {
  const s = P.subs[P.sub]; if (!s) return;
  try {
    const r = await api('PUT', s.key + '/timing', ms === null ? { reset: true } : { delta_ms: ms });
    s.offsetMs = r.offsetMs || null;
    reloadSub(P.sub);
    toast(ms === null ? t('Subtitle timing reset') : t('Subtitles moved by {s} s', { s: fmtNum(r.offsetMs / 1000, 1) }), 3000);
    playerMenu('subs', 'again');
  } catch (e) { toast(e.message, 5000); }
}
const subName = (s, i) => [s.language ? langName(s.language) : '', s.title, s.forced && !/forced|erzwungen/i.test(s.title || '') ? t('forced') : '', s.sdh && !/sdh|hearing|schwerh/i.test(s.title || '') ? 'SDH' : '', !s.language && s.file ? s.file : ''].filter(Boolean).join(' · ') || t('Subtitles {n}', { n: i + 1 });
// The subtitle to switch on for a language: forced ones only when `forced` is asked for (else they are skipped
// unless they are all there is); `sdh` is 'any', 'prefer' (for the hard of hearing first) or 'avoid'. -1 = none.
function pickSub(subs, lang, sdh, forced) {
  const all = subs.map((s, i) => ({ s, i })).filter(x => lang2(x.s.language) === lang);
  let pool = forced ? all.filter(x => x.s.forced) : all.filter(x => !x.s.forced);
  if (!forced && !pool.length) pool = all;
  if (!pool.length) return -1;
  const rank = x => (sdh === 'prefer' ? (x.s.sdh ? 0 : 1) : sdh === 'avoid' ? (x.s.sdh ? 1 : 0) : 0);
  return pool.map(x => [rank(x), x.i]).sort((p, q) => p[0] - q[0] || p[1] - q[1])[0][1];
}
function langName(code) {
  try { return new Intl.DisplayNames([I18N.lang], { type: 'language' }).of(code.length === 3 ? ({ ger: 'de', deu: 'de', eng: 'en', fre: 'fr', fra: 'fr', spa: 'es', ita: 'it', jpn: 'ja', por: 'pt', rus: 'ru', dut: 'nl', nld: 'nl', pol: 'pl', tur: 'tr', swe: 'sv', dan: 'da', nor: 'no', fin: 'fi', kor: 'ko', chi: 'zh', zho: 'zh' }[code] || code) : code) || code; } catch (e) { return code; }
}
// "ger", "deu", "de-DE" → "de" (for comparing a stream's language with a preference).
const L3 = { ger: 'de', deu: 'de', eng: 'en', fre: 'fr', fra: 'fr', spa: 'es', ita: 'it', jpn: 'ja', por: 'pt', rus: 'ru', dut: 'nl', nld: 'nl', pol: 'pl', tur: 'tr', swe: 'sv', dan: 'da', nor: 'no', fin: 'fi', zho: 'zh', chi: 'zh', kor: 'ko' };
const lang2 = c => { c = String(c || '').toLowerCase().split(/[-_]/)[0]; return c.length === 3 ? (L3[c] || c) : c; };
function showSub() {
  Array.from(video.textTracks).forEach((tr, i) => {
    tr.mode = i === P.sub ? 'showing' : 'disabled';
    if (!tr._lifted) { tr._lifted = true; tr.addEventListener('cuechange', () => { // Settings → Web → subtitle position: lines above the bottom
      const up = Number(store.get('subLift', '0')) || 0;
      if (up) for (const c of Array.from(tr.activeCues || [])) { c.snapToLines = true; c.line = -(1 + up); }
    }); }
  });
  $('#psubs').classList.toggle('on', P.sub >= 0 || P.burn !== null);
  $('#psubs').hidden = !P.subs.length;
}
const PICTURE_SUBS = ['pgs', 'dvd_sub', 'dvb_sub', 'xsub', 'vobsub', 'hdmv'];
const isPictureSub = s => !!s && s.embedded && PICTURE_SUBS.some(k => String(s.codec || '').toLowerCase().includes(k));
function setSub(i) {
  const before = P.burn;
  P.burn = null;
  if (i >= 0 && isPictureSub(P.subs[i])) {
    if (store.get('burnSubs', 'auto') !== 'auto') { toast(t('This subtitle is made of pictures; turn on burning in under Settings → Web.'), 5000); i = -1; }
    else { P.burn = i; P.sub = -1; P.lastSub = i; store.set('subLang', P.subs[i].language || P.subs[i].file || ''); showSub(); if (before !== P.burn) load(position()); return; }
  }
  P.sub = i;
  if (i >= 0) { P.lastSub = i; store.set('subLang', P.subs[i].language || P.subs[i].file || ''); } else store.set('subLang', '');
  showSub();
  if (before !== null) load(position()); // the burned-in picture has to go
}
function setSubSize(v) { P.subSize = v; store.set('subSize', v); player.style.setProperty('--cue', v + '%'); }
setSubSize(P.subSize);
function applySubStyle() {
  player.style.setProperty('--cue-color', store.get('subColor', '#ffffff'));
  player.style.setProperty('--cue-bg', store.get('subBg', '1') === '1' ? 'rgba(0,0,0,.55)' : 'transparent');
}
applySubStyle();

// The answer can say the owner stopped this playback (from the dashboard): then
// the player closes and shows their message.
function report(state, atMs) {
  if (!P.it) return Promise.resolve(null);
  const it = P.it, ms = atMs !== undefined ? atMs : Math.floor(position() * 1000);
  P.reported = state;
  return api('POST', `/:/timeline?ratingKey=${it.id}&time=${ms}&state=${state}&client=web`)
    .then(r => { if (r && r.stopped && P.it === it) stoppedByOwner(r.message); return r; })
    .catch(() => null);
}
function stoppedByOwner(message) {
  closePlayer(false);
  const box = modal(t('Playback stopped'), [
    h('p', {}, t('The owner of this server stopped your playback.')),
    message ? h('blockquote', { class: 'panel', style: 'margin:0 0 12px;padding:10px 14px;white-space:pre-wrap' }, message) : null,
    h('div', { class: 'actions' }, h('button', { class: 'primary', onclick: () => box.remove() }, t('OK')))]);
}

// ---- drawing
const pct = s => { const d = duration(); return d ? Math.max(0, Math.min(100, s / d * 100)) : 0; };
function drawBar() {
  if (!P.it) return;
  const now = shown();
  $('.fill', bar).style.width = pct(now) + '%';
  $('.knob', bar).style.left = pct(now) + '%';
  // The buffered stretch around the playing point (in whole-video time).
  const b = video.buffered, ct = video.currentTime, base = (P.base || 0) + P.offset;
  let from = 0, to = 0;
  for (let i = 0; i < b.length; i++) if (b.start(i) <= ct + 0.5 && b.end(i) >= ct) { from = base + b.start(i); to = base + b.end(i); }
  $('.buf', bar).style.left = pct(from) + '%';
  $('.buf', bar).style.width = Math.max(0, pct(to) - pct(from)) + '%';
  const d = duration();
  $('#ptime').textContent = P.remaining && d ? `−${fmtTime(d - now)}` : `${fmtTime(now)} / ${fmtTime(d)}`;
  const ends = d && !video.paused ? new Date(Date.now() + (d - now) / (video.playbackRate || 1) * 1000) : null;
  $('#psub').textContent = [P.subLine, ends ? t('ends at {time}', { time: ends.toLocaleTimeString(I18N.lang, { hour: '2-digit', minute: '2-digit', hour12: hour12() }) }) : ''].filter(Boolean).join(' · ');
  // "Skip credits" while the credits run.
  const inCredits = P.credits.some(c => now >= c) && now < d - 3;
  const skip = $('#pskip');
  if (inCredits && $('#pnext').style.display !== 'flex') {
    const next = P.queue[0] || Q.list[0] || P.next;
    skip.textContent = !next ? t('Skip credits') : next.type === 'episode' ? t('Next episode ▸') : t('Play next ▸');
    skip.style.display = 'block';
  } else skip.style.display = 'none';
  // "Skip intro" while the intro runs.
  const si = $('#pskipintro');
  if (P.intro && now >= P.intro[0] && now < P.intro[1] - 1) { si.textContent = t('Skip intro'); si.style.display = 'block'; }
  else si.style.display = 'none';
}
function drawMarks() {
  const marks = $('.marks', bar); marks.replaceChildren();
  // drawn once the length is known (duration() needs the item)
  // The chapters of the file: a thin line each (the first one is the start), their names show when hovering.
  if ((P.chapters || []).length > 1) for (const c of P.chapters.slice(1)) marks.append(h('div', { class: 'mark chap', style: `left:${pct(c.at)}%` }));
  for (const c of P.credits) marks.append(h('div', { class: 'mark', style: `left:${pct(c)}%`, title: t('Credits') }));
  if (P.intro) marks.append(h('div', { class: 'mark intro', style: `left:${pct(P.intro[0])}%;width:${Math.max(0.3, pct(P.intro[1]) - pct(P.intro[0]))}%`, title: t('Intro') }));
}
function drawButtons() {
  const pb = $('#pplay'), playing = !video.paused;
  pb.replaceChildren(icon(playing ? 'pause' : 'play'));
  pb.title = (playing ? t('Pause') : t('Play')) + ' (K)';
  $('#pmute').replaceChildren(icon(video.muted || video.volume === 0 ? 'mute' : video.volume < 0.5 ? 'low' : 'vol'));
  $('#pmute').title = (video.muted ? t('Sound on') : t('Mute')) + ' (M)';
  $('#pvolume').value = video.muted ? 0 : video.volume;
  const fs = !!document.fullscreenElement;
  btn('pfull', fs ? 'exitfull' : 'full', (fs ? t('Leave full screen') : t('Full screen')) + ' (F)');
}
function flash(name, text) {
  const f = $('#pflash'); f.replaceChildren(text && name !== 'play' && name !== 'pause' ? text : icon(name));
  f.classList.remove('show'); void f.offsetWidth; f.classList.add('show');
}
function spin(on) { $('#pspin').style.display = on ? 'block' : 'none'; }

// The controls fade out while playing and the mouse rests.
let idleTimer = null;
function wake() {
  player.classList.remove('idle');
  clearTimeout(idleTimer);
  idleTimer = setTimeout(() => { if (!video.paused && !P.menu && !bar.classList.contains('drag') && $('#pkeys').style.display !== 'block') player.classList.add('idle'); }, 3000);
}
player.addEventListener('mousemove', wake);
player.addEventListener('touchstart', wake, { passive: true });

// ---- video events
video.addEventListener('play', () => { drawButtons(); wake(); });
// A new source (another quality, or a jump in a converted video) pauses the old one
// on its way: that is no pause (readyState is 0 then). Playing again is told at once.
video.addEventListener('pause', () => { drawButtons(); wake(); if (!video.ended && video.readyState > 0) report('paused'); });
video.addEventListener('playing', () => { if (P.reported !== 'playing') report('playing'); });
video.addEventListener('volumechange', () => { drawButtons(); store.set('volume', video.volume); store.set('muted', video.muted ? '1' : '0'); });
video.addEventListener('timeupdate', () => { if (!bar.classList.contains('drag')) drawBar(); drawLyrics(); });
video.addEventListener('progress', drawBar);
video.addEventListener('durationchange', drawMarks);
video.addEventListener('waiting', () => spin(true));
video.addEventListener('seeking', () => spin(true));
['playing', 'canplay', 'loadeddata', 'seeked', 'pause', 'error'].forEach(ev => video.addEventListener(ev, () => spin(false)));
// A button clicked with the mouse lets go of the focus, so Space keeps meaning play / pause.
player.addEventListener('pointerup', e => { const b = e.target.closest('.pbtn'); if (b && e.pointerType === 'mouse') setTimeout(() => b.blur(), 0); });
video.addEventListener('error', () => {
  if (!P.it || !video.getAttribute('src')) return;
  // Stopped by the owner? Then the report says so (and shows why).
  report('playing').then(r => { if (!(r && r.stopped)) toast(t('This video could not be played. Try a lower quality in ⚙.'), 6000); });
});
video.addEventListener('ended', () => {
  // The next part of a split movie.
  if (split() && P.part < P.parts.length - 1) { load(partStart(P.part + 1)); return; }
  report('stopped', Math.floor(duration() * 1000));
  upNext();
});
// The end, or "Skip credits": the next one after a countdown, or close.
function upNext() {
  const next = P.queue[0] || Q.list[0] || P.next;
  if (!next) return closePlayer(false);
  if (!P.autoNext || (P.suggest && next === P.next)) return showNext(next, null);
  // Like Netflix: after a few episodes in a row without anyone touching anything, ask if somebody is still there.
  if ((P.autoRun || 0) >= STILL_WATCHING_AFTER) return showStillWatching(next);
  showNext(next, 10);
}
const STILL_WATCHING_AFTER = 3, STILL_WATCHING_WAIT = 30;
// Any input (not just moving the mouse) shows that somebody is there.
for (const ev of ['pointerdown', 'keydown', 'touchstart', 'wheel']) document.addEventListener(ev, () => { P.autoRun = 0; }, { passive: true, capture: true });
function showStillWatching(next) {
  const box = $('#pnext'), count = h('span');
  clearInterval(P.countdown);
  const stop = () => { hideNext(); closePlayer(false); };
  const go = h('button', { class: 'primary', onclick: () => { P.autoRun = 0; playNext(); } }, t('Yes, keep watching'));
  box.replaceChildren(h('img', { src: withTok(next.thumb || `/library/metadata/${next.id}/thumb`), alt: '' }),
    h('div', {}, h('b', {}, t('Are you still watching?')), h('div', { class: 'dim' }, nextName(next), count),
      h('div', { class: 'actions' }, go, h('button', { onclick: stop }, t('Stop')))));
  box.style.display = 'flex';
  $('#pskip').style.display = 'none';
  wake();
  let left = STILL_WATCHING_WAIT;
  const tick = () => { count.textContent = ' · ' + t('stops in {n} s', { n: left }); if (left-- <= 0) stop(); };
  tick();
  P.countdown = setInterval(tick, 1000);
  go.focus();
}
function showNext(next, secs) {
  const box = $('#pnext'), count = h('span');
  clearInterval(P.countdown);
  const playNow = h('button', { class: 'primary', onclick: () => playNext() }, t('Play now'));
  box.replaceChildren(h('img', { src: withTok(next.thumb || `/library/metadata/${next.id}/thumb`), alt: '' }),
    h('div', {}, h('div', { class: 'dim' }, t('Up next'), count), h('b', {}, next.type === 'episode' ? next.grandparentTitle || '' : next.title),
      next.type === 'episode' ? h('div', {}, nextName(next)) : null,
      h('div', { class: 'actions' }, playNow, h('button', { onclick: () => { hideNext(); if (video.ended) closePlayer(false); } }, t('Cancel')))));
  box.style.display = 'flex';
  $('#pskip').style.display = 'none';
  wake();
  if (secs) {
    let left = secs;
    const tick = () => { count.textContent = ' · ' + t('in {n} s', { n: left }); if (left-- <= 0) { P.autoRun = (P.autoRun || 0) + 1; playNext(); } };
    tick();
    P.countdown = setInterval(tick, 1000);
  }
  playNow.focus();
}
function hideNext() { clearInterval(P.countdown); P.countdown = null; $('#pnext').style.display = 'none'; }
$('#pskipintro').addEventListener('click', () => {
  $('#pskipintro').style.display = 'none';
  if (P.intro) seekTo(P.intro[1]).then(() => tgSend());
});
$('#pskip').addEventListener('click', () => {
  $('#pskip').style.display = 'none';
  // Counts as watched to the end, like letting the credits run.
  report('stopped', Math.floor(duration() * 1000));
  if (P.queue.length || Q.list.length || P.next) playNext(); else closePlayer(false);
});

// ---- seek bar: click, drag, hover preview
const barTime = e => { const r = bar.getBoundingClientRect(); return Math.max(0, Math.min(1, (e.clientX - r.left) / r.width)) * duration(); };
let frameTimer = null, frameWant = null;
function hoverTip(e) {
  const s = barTime(e), tip = $('.tip', bar), r = bar.getBoundingClientRect();
  const half = 104, x = Math.max(half - 4, Math.min(r.width - half + 4, e.clientX - r.left));
  tip.style.left = x + 'px';
  const chap = (P.chapters || []).length > 1 ? [...P.chapters].reverse().find(c => c.at <= s) : null;
  $('span', tip).textContent = fmtTime(s) + (chap ? ' · ' + chap.title : '');
  // A still from the server (rounded to 10 s), asked for once the pointer rests a moment.
  const img = $('img', tip), step = Math.floor(s / 10) * 10;
  if (frameWant === step) return;
  frameWant = step;
  clearTimeout(frameTimer);
  frameTimer = setTimeout(() => {
    if (!P.it || !P.it.Media) return;
    const src = withTok(`/library/parts/${P.it.id}/frame?t=${step}`);
    const pre = new Image();
    pre.onload = () => { if (frameWant === step) { img.src = src; img.style.display = 'block'; } };
    pre.onerror = () => { if (frameWant === step) img.style.display = 'none'; };
    pre.src = src;
  }, 120);
}
bar.addEventListener('pointermove', e => {
  hoverTip(e);
  if (bar.classList.contains('drag')) { P.pending = barTime(e); drawBar(); }
});
bar.addEventListener('pointerleave', () => { frameWant = null; $('.tip img', bar).style.display = 'none'; });
bar.addEventListener('pointerdown', e => {
  if (!duration()) return;
  bar.setPointerCapture(e.pointerId);
  bar.classList.add('drag');
  P.pending = barTime(e); drawBar();
});
bar.addEventListener('pointerup', async e => {
  if (!bar.classList.contains('drag')) return;
  bar.classList.remove('drag');
  const to = barTime(e);
  await seekTo(to);
  if (P.method !== 'direct' || split()) tgSend();
  wake();
});

// ---- buttons
const togglePlay = () => { if (video.paused) { video.play().catch(() => {}); flash('play'); } else { video.pause(); flash('pause'); } };
$('#pplay').addEventListener('click', togglePlay);
$('#pback').addEventListener('click', () => nudge(-10));
$('#pfwd').addEventListener('click', () => nudge(30));
$('#pnextbtn').addEventListener('click', () => { report('stopped'); playNext(); });
$('#pmute').addEventListener('click', () => { video.muted = !video.muted; if (!video.muted && video.volume === 0) video.volume = 0.5; });
// The last and first bit of the track count as full and silent (the thumb's travel is shorter than the track).
// Dragging works from the pointer's position on the track, also past either end (so 100 % and 0 % are easy to hit),
// and keeps the controls awake meanwhile.
{
  const vol = $('#pvolume');
  const setFrom = e => {
    const r = vol.getBoundingClientRect(), pad = 8; // the thumb's half width
    let v = Math.max(0, Math.min(1, (e.clientX - r.left - pad) / Math.max(1, r.width - 2 * pad)));
    if (v > 0.96) v = 1; if (v < 0.03) v = 0;
    vol.value = v; video.volume = v; video.muted = v === 0;
  };
  vol.addEventListener('pointerdown', e => { if (e.button) return; e.preventDefault(); vol.focus(); vol.setPointerCapture(e.pointerId); vol.dataset.drag = '1'; setFrom(e); wake(); });
  vol.addEventListener('pointermove', e => { if (vol.dataset.drag) { setFrom(e); wake(); } });
  const done = e => { delete vol.dataset.drag; wake(); };
  vol.addEventListener('pointerup', done); vol.addEventListener('pointercancel', done);
}
$('#pvolume').addEventListener('input', e => { let v = Number(e.target.value); if (v > 0.96) v = 1; if (v < 0.03) v = 0; video.volume = v; video.muted = v === 0; });
$('#ptime').addEventListener('click', () => { P.remaining = !P.remaining; store.set('timeLeft', P.remaining ? '1' : '0'); drawBar(); });
$('#pfull').addEventListener('click', toggleFull);
$('#ppip').addEventListener('click', togglePip);
function toggleFull() {
  if (document.fullscreenElement) document.exitFullscreen().catch(() => {});
  else player.requestFullscreen().catch(() => toast(t('Full screen is not available here.')));
}
document.addEventListener('fullscreenchange', drawButtons);
function togglePip() {
  if (!document.pictureInPictureEnabled) return;
  if (document.pictureInPictureElement) document.exitPictureInPicture().catch(() => {});
  else video.requestPictureInPicture().catch(e => toast(e.message));
}
// A click on the picture plays or pauses; a double click switches full screen.
let clickTimer = null;
video.addEventListener('click', () => {
  if (P.menu) return closePlayerMenu();
  clearTimeout(clickTimer);
  clickTimer = setTimeout(togglePlay, 220);
});
video.addEventListener('dblclick', () => { clearTimeout(clickTimer); toggleFull(); });

// ---- menus (subtitles, and ⚙ with quality, speed, audio, subtitle size, autoplay)
function closePlayerMenu() { P.menu = null; $('#pmenu').style.display = 'none'; }
function playerMenu(name, page) {
  const m = $('#pmenu');
  if (P.menu === name && !page) return closePlayerMenu();
  P.menu = name;
  const choice = (label, on, pick) => h('div', { class: 'mi', tabindex: 0, role: 'menuitemradio', 'aria-checked': on ? 'true' : 'false',
    onclick: e => { e.stopPropagation(); pick(); }, onkeydown: e => { if (e.key === 'Enter' || e.key === ' ') { e.preventDefault(); pick(); } } },
    h('span', { class: 'ck' }, on ? '✓' : ''), label);
  const entry = (label, value, go) => h('div', { class: 'mi', tabindex: 0, role: 'menuitem', onclick: e => { e.stopPropagation(); go(); },
    onkeydown: e => { if (e.key === 'Enter' || e.key === ' ') { e.preventDefault(); go(); } } }, label, h('span', { class: 'v' }, value + ' ›'));
  const back = title => h('div', { class: 'mh back', onclick: e => { e.stopPropagation(); playerMenu('settings', 'main'); } }, '‹ ' + title);
  let kids = [];
  if (name === 'subs') {
    kids = [h('div', { class: 'mh' }, t('Subtitles')), choice(t('Off'), P.sub < 0 && P.burn === null, () => { setSub(-1); closePlayerMenu(); }),
      ...P.subs.map((s, i) => choice(subName(s, i) + (isPictureSub(s) ? ' · ' + t('burned in') : ''), P.sub === i || P.burn === i, () => { setSub(i); closePlayerMenu(); })),
      h('div', { class: 'mh' }, t('Subtitle size')), ...SUB_SIZES.map(([v, l]) => choice(l, P.subSize === v, () => { setSubSize(v); playerMenu('subs', 'again'); })),
      ...(S.me.admin && P.sub >= 0 && P.subs[P.sub] && !P.part ? [h('div', { class: 'mh' }, t('Subtitle timing') + (P.subs[P.sub].offsetMs ? ' · ' + fmtNum(P.subs[P.sub].offsetMs / 1000, 1) + ' s' : '')),
        h('div', { class: 'mi', tabindex: 0, onclick: e => { e.stopPropagation(); syncSub(); } }, h('span', { class: 'ck' }), t('Synchronize with the sound')),
        h('div', { class: 'mi', tabindex: 0, onclick: e => { e.stopPropagation(); nudgeSub(-500); } }, h('span', { class: 'ck' }), t('Earlier by 0.5 s')),
        h('div', { class: 'mi', tabindex: 0, onclick: e => { e.stopPropagation(); nudgeSub(500); } }, h('span', { class: 'ck' }), t('Later by 0.5 s')),
        P.subs[P.sub].offsetMs ? h('div', { class: 'mi', tabindex: 0, onclick: e => { e.stopPropagation(); nudgeSub(null); } }, h('span', { class: 'ck' }), t('Reset timing')) : null] : [])];
  } else if (!page || page === 'main') {
    const q = QUALITIES.find(x => x[0] === P.quality), a = P.audios.find(x => String(x.index) === String(P.audio));
    kids = [entry(t('Quality'), q ? q[1] : P.quality, () => playerMenu('settings', 'quality')),
      entry(t('Playback speed'), P.speed === 1 ? t('Normal speed') : fmtNum(P.speed) + '×', () => playerMenu('settings', 'speed')),
      P.audios.length > 1 ? entry(t('Audio track'), a ? audioName(a) : t('Default audio'), () => playerMenu('settings', 'audio')) : null,
      // Makes no sense for a single movie: only where something follows (episodes, a playlist or queue).
      P.it && (P.it.type === 'episode' || P.queue.length || Q.list.length) ? choice(t('Play the next episode by itself'), P.autoNext, () => { P.autoNext = !P.autoNext; store.set('autoNext', P.autoNext ? '1' : '0'); playerMenu('settings', 'main'); }) : null,
      h('div', { class: 'mi', tabindex: 0, onclick: e => { e.stopPropagation(); closePlayerMenu(); showKeys(); } }, h('span', { class: 'ck' }), t('Keyboard shortcuts'), h('span', { class: 'v' }, '?'))];
  } else if (page === 'quality') {
    kids = [back(t('Quality')), ...QUALITIES.map(([v, l]) => choice(l, P.quality === v, () => { P.quality = v; store.set('quality', v); closePlayerMenu(); load(position()); }))];
  } else if (page === 'speed') {
    kids = [back(t('Playback speed')), ...SPEEDS.map(v => choice(v === 1 ? t('Normal speed') : fmtNum(v) + '×', P.speed === v, () => { setSpeed(v); playerMenu('settings', 'speed'); }))];
  } else if (page === 'audio') {
    kids = [back(t('Audio track')), choice(t('Default audio'), P.audio === '', () => { P.audio = ''; closePlayerMenu(); load(position()); }),
      ...P.audios.map(a => choice(audioName(a), String(P.audio) === String(a.index), () => { P.audio = a.index; closePlayerMenu(); load(position()); }))];
  }
  m.replaceChildren(...kids.filter(Boolean));
  m.style.display = 'block';
  const first = m.querySelector('.mi'); if (first && page !== 'again') first.focus({ preventScroll: true });
  wake();
}
const audioName = a => [a.language ? langName(a.language) : t('audio'), (a.codec || '').toUpperCase(), a.channels ? a.channels + 'ch' : '', a.title || ''].filter(Boolean).join(' · ');
$('#psubs').addEventListener('click', e => { e.stopPropagation(); playerMenu('subs'); });
$('#psettings').addEventListener('click', e => { e.stopPropagation(); playerMenu('settings'); });
player.addEventListener('click', e => { if (P.menu && !e.target.closest('#pmenu')) closePlayerMenu(); });

function setSpeed(v) {
  P.speed = v;
  store.set('speed', v);
  video.defaultPlaybackRate = video.playbackRate = v;
}
// The browser's own menu (picture-in-picture …) can change the speed too: keep it in step.
video.addEventListener('ratechange', () => { if (P.it && SPEEDS.includes(video.playbackRate) && video.playbackRate !== P.speed) setSpeed(video.playbackRate); });

// ---- the system's media keys and lock screen
function mediaSession(it) {
  if (!('mediaSession' in navigator)) return;
  try {
    navigator.mediaSession.metadata = new MediaMetadata({ title: it.title, artist: it.type === 'episode' ? `${it.grandparentTitle} · ${t('S{n}', { n: it.parentIndex })}${t('E{n}', { n: it.index })}` : it.type === 'track' ? (it.grandparentTitle || '') : (it.year ? String(it.year) : ''), album: it.type === 'track' ? (it.parentTitle || '') : '',
      artwork: [{ src: withTok(it.type === 'episode' && it.grandparentThumb ? it.grandparentThumb : it.thumb), sizes: '300x450', type: 'image/jpeg' }] });
    const on = (a, f) => { try { navigator.mediaSession.setActionHandler(a, f); } catch (e) { /* not offered by this browser */ } };
    on('play', () => video.play()); on('pause', () => video.pause());
    on('seekbackward', () => nudge(-10)); on('seekforward', () => nudge(30));
    on('seekto', d => seekTo(d.seekTime)); on('nexttrack', () => { if (P.next || P.queue.length || Q.list.length) playNext(); });
  } catch (e) { /* older browser */ }
}

function closePlayer(send) {
  if (send) report('stopped');
  tgLeave();
  clearInterval(P.timer); clearTimeout(P.pendingTimer); hideNext(); closePlayerMenu();
  if (document.fullscreenElement) document.exitFullscreen().catch(() => {});
  if (document.pictureInPictureElement) document.exitPictureInPicture().catch(() => {});
  video.pause(); video.removeAttribute('src'); video.querySelectorAll('track').forEach(tr => tr.remove()); video.load();
  P.lyrics = null; $('#plyrics').style.display = 'none'; $('#plyricsbtn').hidden = true;
  player.style.display = 'none';
  player.classList.remove('idle');
  P.it = null; P.next = null; P.pending = null; P.autoRun = 0;
  if ('mediaSession' in navigator) navigator.mediaSession.metadata = null;
  render();
}
$('#pclose').addEventListener('click', () => closePlayer(true));

// ---- keyboard
const KEYS = [['Space / K', 'Play or pause'], ['← / J', 'Back 10 seconds'], ['→ / L', 'Forward 30 seconds'], ['Shift + ← / →', '1 minute back / forward'],
  ['↑ / ↓', 'Louder / quieter'], ['M', 'Mute'], ['F', 'Full screen'], ['P', 'Picture in picture'], ['C', 'Subtitles on / off'], ['Y', 'Lyrics on / off'], ['N', 'Next'],
  ['< / >', 'Slower / faster'], ['0 … 9', 'Jump to 0 % … 90 %'], ['Home / End', 'Start / near the end'], ['?', 'This list'], ['Esc', 'Close']];
function showKeys() {
  const k = $('#pkeys');
  k.replaceChildren(h('h2', { style: 'margin-top:0' }, t('Keyboard shortcuts')),
    h('table', {}, KEYS.map(([key, what]) => h('tr', {}, h('td', {}, key.split(' ').map(x => /^[\w←→↑↓<>?…\/+%]+$/.test(x) && !['/', '+', '…'].includes(x) ? h('kbd', {}, x) : ' ' + x + ' ')), h('td', {}, t(what))))),
    h('div', { class: 'actions' }, h('button', { onclick: () => { k.style.display = 'none'; } }, t('Close'))));
  k.style.display = 'block';
}
document.addEventListener('keydown', e => {
  if (!P.it || player.style.display !== 'flex') return;
  if (/^(INPUT|SELECT|TEXTAREA)$/.test(e.target.tagName) && e.target.type !== 'range') return;
  if (e.ctrlKey || e.metaKey || e.altKey) return;
  const k = e.key, inMenu = e.target.closest && e.target.closest('#pmenu');
  if (k === 'Escape') {
    if ($('#pkeys').style.display === 'block') { $('#pkeys').style.display = 'none'; return; }
    if (P.menu) return closePlayerMenu();
    if ($('#pnext').style.display === 'flex') return hideNext();
    return closePlayer(true);
  }
  if (inMenu && (k === 'Enter' || k === ' ')) return;
  if (inMenu && (k === 'ArrowDown' || k === 'ArrowUp')) {
    e.preventDefault();
    const items = Array.from($('#pmenu').querySelectorAll('.mi')), i = items.indexOf(document.activeElement);
    const n = items[(i + (k === 'ArrowDown' ? 1 : -1) + items.length) % items.length]; if (n) n.focus();
    return;
  }
  // Buttons keep Enter for themselves; Space always plays or pauses.
  if (k === 'Enter' && e.target.tagName === 'BUTTON') return;
  let done = true;
  switch (k) {
    case ' ': case 'k': case 'K': togglePlay(); break;
    case 'ArrowLeft': case 'j': case 'J': nudge(e.shiftKey ? -60 : -10); break;
    case 'ArrowRight': case 'l': case 'L': nudge(e.shiftKey ? 60 : 30); break;
    case 'ArrowUp': video.muted = false; video.volume = Math.min(1, Math.round((video.volume + 0.05) * 100) / 100); toast(t('Volume: {n} %', { n: Math.round(video.volume * 100) }), 900); break;
    case 'ArrowDown': video.volume = Math.max(0, Math.round((video.volume - 0.05) * 100) / 100); toast(t('Volume: {n} %', { n: Math.round(video.volume * 100) }), 900); break;
    case 'm': case 'M': video.muted = !video.muted; flash(video.muted ? 'mute' : 'vol'); break;
    case 'f': case 'F': toggleFull(); break;
    case 'p': case 'P': togglePip(); break;
    case 'c': case 'C':
      if (!P.subs.length) { toast(t('No subtitles for this video')); break; }
      setSub(P.sub >= 0 ? -1 : (P.lastSub !== undefined ? P.lastSub : 0));
      toast(P.sub >= 0 ? t('Subtitles: {name}', { name: subName(P.subs[P.sub], P.sub) }) : t('Subtitles off'));
      break;
    case 'y': case 'Y': toggleLyrics(); break;
    case 'n': case 'N': if (P.next || P.queue.length || Q.list.length) { report('stopped'); playNext(); } break;
    case 'Home': seekTo(0); break;
    case 'End': seekTo(duration() - 15); break;
    case '?': showKeys(); break;
    case '<': case '>': {
      // < and > step the speed, as in many players.
      const i = SPEEDS.indexOf(P.speed), next = SPEEDS[Math.max(0, Math.min(SPEEDS.length - 1, (i < 0 ? 2 : i) + (k === '>' ? 1 : -1)))];
      setSpeed(next);
      toast(t('Speed: {speed}', { speed: next === 1 ? t('Normal speed') : fmtNum(next) + '×' }));
      break;
    }
    default:
      if (/^[0-9]$/.test(k) && duration()) seekTo(duration() * Number(k) / 10).then(() => tgSend());
      else done = false;
  }
  if (done) { e.preventDefault(); wake(); }
});
drawButtons();
