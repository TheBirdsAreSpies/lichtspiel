// Lichtspiel web app, part 1: helpers, sign-in, browsing, details, player.
// Talks only to the server's JSON API (see #/api). Part 2 is pages.js.
// Every text shown goes through t() (i18n.js); the language packs are in web/lang/.
'use strict';
// A link with ?token=… (made by the status window and the self-test) signs this browser in once,
// then the token is taken out of the address bar.
(() => {
  const u = new URL(location.href); const tok = u.searchParams.get('token');
  if (tok) { localStorage.setItem('token', tok); u.searchParams.delete('token'); history.replaceState(null, '', u.pathname + u.search + u.hash); }
})();
const S = { token: localStorage.getItem('token') || '', me: null, sections: [], filters: null, status: null };
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
function set(...kids) { main().replaceChildren(...kids.flat(9).filter(Boolean)); window.scrollTo(0, 0); }
function toast(msg, ms = 2600) { const el = $('#toast'); el.textContent = msg; el.style.display = 'block'; clearTimeout(el._t); el._t = setTimeout(() => (el.style.display = 'none'), ms); }
const withTok = p => p + (p.includes('?') ? '&' : '?') + 'token=' + encodeURIComponent(S.token);
const fmtNum = (n, digits) => Number(n).toLocaleString(I18N.lang, digits === undefined ? undefined : { minimumFractionDigits: digits, maximumFractionDigits: digits });
const fmtDur = ms => { if (!ms) return ''; const m = Math.round(ms / 60000); if (m < 1) return Math.max(1, Math.round(ms / 1000)) + ' s'; return m >= 60 ? t('{h} h {m} min', { h: Math.floor(m / 60), m: m % 60 }) : t('{m} min', { m }); };
const fmtTime = s => { s = Math.max(0, Math.floor(s)); const hh = Math.floor(s / 3600), mm = Math.floor(s / 60) % 60, ss = s % 60; return (hh ? hh + ':' + String(mm).padStart(2, '0') : mm) + ':' + String(ss).padStart(2, '0'); };
const fmtDate = sec => sec ? new Date(sec * 1000).toLocaleDateString(I18N.lang) : '';
const fmtDateTime = sec => sec ? new Date(sec * 1000).toLocaleString(I18N.lang) : '';
const fmtSize = b => b > 1e9 ? fmtNum(b / 1e9, 1) + ' GB' : fmtNum(b / 1e6, 0) + ' MB';

async function api(method, path, body) {
  const opt = { method, headers: { 'X-Token': S.token, Accept: 'application/json' } };
  if (body !== undefined) { opt.headers['Content-Type'] = 'application/json'; opt.body = JSON.stringify(body); }
  const r = await fetch(path, opt);
  let j = null;
  try { j = await r.json(); } catch (e) { /* not JSON */ }
  if (r.status === 401 && S.token && !path.startsWith('/api/login')) { logout(); throw new Error(t('Please sign in again')); }
  if (!r.ok) throw new Error((j && j.error) ? t(j.error) : 'HTTP ' + r.status);
  return j;
}
const GET = p => api('GET', p);
const meta = j => (j && j.MediaContainer && j.MediaContainer.Metadata) || [];

// ---------- sign in, first setup, invites
function logout() { S.token = ''; S.me = null; localStorage.removeItem('token'); location.hash = '#/login'; drawChrome(); render(); }

function authPage(title, intro, fields, button, submit) {
  const err = h('div', { class: 'err' });
  const inputs = {};
  const form = h('form', { class: 'center', onsubmit: async e => {
    e.preventDefault(); err.textContent = '';
    const vals = {}; for (const [k, el] of Object.entries(inputs)) vals[k] = el.value;
    try { await submit(vals); } catch (x) { err.textContent = x.message; }
  } },
    h('h1', {}, title), intro ? h('p', { class: 'dim' }, intro) : null,
    fields.map(([k, label, type, val]) => [h('label', {}, label), (inputs[k] = h('input', { type: type || 'text', value: val || '' }))]),
    h('div', { style: 'margin-top:18px' }, h('button', { class: 'primary', type: 'submit', style: 'width:100%' }, button)), err,
    Object.keys(I18N.packs).length > 1 ? h('div', { style: 'margin-top:14px;text-align:right' }, langSelect()) : null);
  set(form);
  const first = form.querySelector('input'); if (first) first.focus();
}
async function signedIn(token) { S.token = token; localStorage.setItem('token', token); location.hash = '#/'; await boot(); }
function loginPage() {
  authPage(t('Sign in'), S.status ? S.status.name : '', [['username', t('User name')], ['password', t('Password'), 'password']], t('Sign in'),
    async v => { const r = await api('POST', '/api/login', { username: v.username, password: v.password, device: 'web' }); await signedIn(r.token); });
}
function setupPage() {
  authPage(t('Welcome'), t('Create the owner account for this server. You can invite other people afterwards.'),
    [['server_name', t('Server name'), 'text', 'Lichtspiel'], ['username', t('Your user name')], ['password', t('Password (at least 6 characters)'), 'password']], t('Create account'),
    async v => { const r = await api('POST', '/api/setup', v); await signedIn(r.token); });
}
async function invitePage(code) {
  let info;
  try { info = await api('GET', '/api/invites/' + code); } catch (e) { return set(h('div', { class: 'center' }, h('h1', {}, t('Invite')), h('p', { class: 'err' }, e.message), h('a', { href: '#/login' }, t('Sign in instead')))); }
  authPage(t('Join {server}', { server: info.server }), t('{who} invited you{note}. Pick a user name and password.', { who: info.invited_by || t('Someone'), note: info.note ? ': ' + info.note : '' }),
    [['username', t('User name')], ['email', t('Email (optional)'), 'email', info.email], ['password', t('Password (at least 6 characters)'), 'password']], t('Join'),
    async v => { const r = await api('POST', `/api/invites/${code}/accept`, v); await signedIn(r.token); });
}

// ---------- chrome
async function boot() {
  I18N.applyStatic();
  try { S.status = await GET('/api/status'); } catch (e) { S.status = null; }
  if (S.status) { $('#sname').textContent = S.status.name; document.title = S.status.name === 'Lichtspiel' ? 'Lichtspiel' : `${S.status.name} · Lichtspiel`; }
  S.me = null;
  if (S.token) {
    try {
      S.me = await GET('/api/me');
      S.sections = (await GET('/library/sections')).MediaContainer.Directory;
      S.filters = await GET('/api/filters');
    } catch (e) { S.me = null; }
  }
  drawChrome();
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
  who.append(h('span', {}, S.me.username), h('button', { class: 'small', onclick: logout }, t('Sign out')));
  document.querySelectorAll('#side .admin').forEach(e => (e.style.display = S.me.admin ? '' : 'none'));
  $('#navsections').replaceChildren(...S.sections.map(s => h('a', { href: '#/section/' + s.id, 'data-nav': 'section/' + s.id },
    h('span', {}, s.title), h('small', {}, s.type === 'show' ? t('{n} shows', { n: s.count }) : s.videoCount))));
}
function markNav(key) { document.querySelectorAll('#side a').forEach(a => a.classList.toggle('on', a.dataset.nav === key)); }
let searchTimer;
$('#q').addEventListener('input', e => { clearTimeout(searchTimer); searchTimer = setTimeout(() => { const v = e.target.value.trim(); location.hash = v ? '#/search/' + encodeURIComponent(v) : '#/'; }, 300); });

// ---------- cards
function posterCard(it, opts = {}) {
  const prog = it.viewOffset && it.duration ? Math.min(1, it.viewOffset / it.duration) : 0;
  let title = it.title, sub = it.year || '';
  if (it.type === 'episode') { title = it.grandparentTitle || it.title; sub = `${t('S{n}', { n: it.parentIndex || 0 })} ${t('E{n}', { n: it.index || 0 })} · ${it.title}`; }
  if (it.type === 'season') sub = it.parentTitle || '';
  if (it.type === 'show') sub = t('{n} episodes', { n: it.leafCount || 0 });
  if (it.type === 'season') title = seasonTitle(title);
  const unseen = it.leafCount ? it.leafCount - (it.viewedLeafCount || 0) : 0;
  const img = it.type === 'episode' && it.grandparentThumb && !opts.episodeThumb ? it.grandparentThumb : it.thumb;
  const seen = it.viewCount > 0 || (it.leafCount && unseen === 0);
  // opts.pick(e, circle) = the library's multiple selection; true when it took the click.
  return h('div', { class: seen ? 'card watched' : 'card', onclick: e => { if (!(opts.pick && opts.pick(e, false))) location.hash = '#/item/' + it.id; } },
    h('div', { class: 'poster' }, h('img', { src: withTok(img), loading: 'lazy', alt: '' }),
      prog > 0 ? h('div', { class: 'bar' }, h('i', { style: `width:${prog * 100}%` })) : null,
      it.leafCount && unseen > 0 ? h('span', { class: 'badge' }, unseen) : null,
      seen ? h('span', { class: 'seen', title: t('Watched') }, '✓') : null,
      opts.pick ? h('span', { class: 'pick', title: t('Select'), onclick: e => { e.stopPropagation(); opts.pick(e, true); } }) : null),
    h('div', { class: 't', title }, title), h('div', { class: 's' }, sub));
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
      oninput: e => { const v = cardSize(e.target.value); try { localStorage.setItem('cardSize', String(v)); } catch (er) { /* private window */ } } }),
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
  const bar = h('div', { class: 'pickbar' }, count,
    h('button', { class: 'small', onclick: mark(true) }, t('Mark watched')),
    h('button', { class: 'small', onclick: mark(false) }, t('Mark unwatched')),
    playlistMenu,
    h('button', { class: 'small', onclick: () => { items.forEach((_, i) => picked.add(i)); draw(); } }, t('Select all')),
    h('button', { class: 'small', title: t('Clear selection'), onclick: () => clear() }, '✕'));
  const draw = () => {
    cards.forEach((c, i) => c.classList.toggle('picked', picked.has(i)));
    grid.classList.toggle('picking', picked.size > 0);
    count.textContent = picked.size === 1 ? t('1 selected') : t('{n} selected', { n: picked.size });
    bar.classList.toggle('show', picked.size > 0);
    if (picked.size) fillMenu();
  };
  const clear = () => { picked.clear(); last = -1; draw(); };
  const toggle = (i, e) => {
    if (e.shiftKey && last >= 0) { for (let k = Math.min(last, i); k <= Math.max(last, i); k++) picked.add(k); }
    else if (picked.has(i)) picked.delete(i); else picked.add(i);
    last = i; draw();
  };
  const card = i => posterCard(items[i], { ...cardOpts, pick: (e, circle) => {
    if (!circle && !picked.size && !e.ctrlKey && !e.metaKey && !e.shiftKey) return false;
    e.preventDefault(); toggle(i, e); return true;
  } });
  const cards = items.map((_, i) => card(i));
  grid.append(...cards);
  grid._clear = () => { if (!picked.size) return false; clear(); return true; };
  // The bar sits outside the page, so it goes when the page changes.
  document.querySelectorAll('.pickbar').forEach(b => b.remove());
  document.body.append(bar);
  return grid;
}
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
const row = (title, items) => items.length ? [h('h2', {}, t(title)), scroller('row', items.map(i => posterCard(i)))] : null;

// ---------- home, search, browse
async function homePage() {
  markNav('home');
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
const FILTER_NAMES = { decade: 'Decade', content_rating: 'Age rating', collection: 'Collection', director: 'Director', actor: 'Actor', writer: 'Writer', producer: 'Producer',
  country: 'Country', studio: 'Studio', network: 'Network', video_resolution: 'Resolution', hdr: 'HDR', hdr10plus: 'HDR10+', atmos: 'Dolby Atmos', unmatched: 'Unmatched',
  video_codec: 'Video codec', audio_codec: 'Audio codec', subtitle_codec: 'Subtitle format', audio_layout: 'Audio channels', audio_language: 'Audio language',
  subtitle_language: 'Subtitle language', edition: 'Edition', label: 'Label', folder: 'Folder', duplicate: 'Duplicates' };
const RES_NAMES = { '4k': '4K', 1080: '1080p', 720: '720p', 480: '480p', sd: 'SD' };
// Covered by the genre and watched menus in the bar.
const BAR_FILTERS = ['genre', 'unwatched', 'in_progress', 'unwatched_leaves'];

async function sectionPage(id, qs) {
  markNav('section/' + id);
  const sec = S.sections.find(s => String(s.id) === String(id));
  if (!sec) return set(h('div', { class: 'empty' }, t('Library not found.')));
  const p = new URLSearchParams(qs || '');
  // The library's own pages: Library, Collections, Playlists, Categories.
  const tab = p.get('tab') || '';
  const tabs = h('div', { class: 'tabs' }, [['', t('Library')], ['collections', t('Collections')], ['playlists', t('Playlists')], ['categories', t('Categories')]]
    .map(([k, l]) => h('a', { href: `#/section/${id}${k ? '?tab=' + k : ''}`, class: tab === k ? 'on' : '' }, l)));
  if (tab) return sectionSubPage(sec, tab, tabs);
  const st = {}; for (const k of ['type', 'sort', 'genre', 'watched', 'from', 'to', 'q', 'container', 'more']) st[k] = p.get(k) || '';
  for (const [k, v] of p) if (k.startsWith('f.')) st[k] = v;
  if (!st.sort) st.sort = 'title';
  const go = () => { const n = new URLSearchParams(); for (const [k, v] of Object.entries(st)) if (v) n.set(k, v); location.hash = `#/section/${id}?${n}`; };
  const genres = (await GET(`/library/sections/${id}/genres`)).MediaContainer.Directory;
  const filters = st.more ? (await GET(`/library/sections/${id}/filters?includeAdvanced=1${st.type ? '&type=' + st.type : ''}`)).MediaContainer.Filter.filter(f => !BAR_FILTERS.includes(f.filter)) : [];
  const q = new URLSearchParams();
  if (st.type) q.set('type', st.type);
  q.set('sort', st.sort);
  if (st.genre) q.set('genre', st.genre);
  if (st.watched === 'unwatched') q.set('watched', 'false');
  if (st.watched === 'watched') q.set('watched', 'true');
  if (st.watched === 'progress') q.set('in_progress', 'true');
  if (st.from) q.set('year>', st.from);
  if (st.to) q.set('year<', st.to);
  if (st.q) q.set('q', st.q);
  if (st.container) q.set('container', st.container);
  for (const [k, v] of Object.entries(st)) if (k.startsWith('f.') && v) q.set(k.slice(2), v);
  const active = Object.keys(st).filter(k => k.startsWith('f.') && st[k]).length;
  const res = (await GET(`/library/sections/${id}/all?${q}`)).MediaContainer;
  const sel = (key, opts) => h('select', { onchange: e => { st[key] = e.target.value; if (key === 'type') for (const k of Object.keys(st)) if (k.startsWith('f.')) delete st[k]; go(); } }, opts.map(([v, l]) => h('option', { value: v, selected: st[key] === v }, l)));
  const valueName = (f, v) => f.filter === 'video_resolution' ? (RES_NAMES[v] || v) : f.filter === 'decade' ? t('{v}s', { v }) : v;
  const filterBox = f => {
    const key = 'f.' + f.filter;
    const opts = f.type === 'boolean' ? [['', t('Any')], ['true', t('Yes ({n})', { n: f.count })], ['false', t('No')]] : [['', f.values.length ? t('Any') : t('None found')], ...f.values.map(x => [x.value, `${valueName(f, x.value)} (${x.count})`])];
    if (st[key] && !opts.some(o => o[0] === st[key])) opts.push([st[key], st[key]]);
    return h('label', { class: st[key] ? 'on' : '' }, t(FILTER_NAMES[f.filter] || f.title), sel(key, opts));
  };
  // "Duplicates" stays apart as its only advanced filter.
  const advanced = filters.filter(f => f.advanced);
  const panel = st.more ? h('div', { class: 'filter-panel' }, filters.filter(f => !f.advanced).map(filterBox),
    advanced.length ? h('div', { class: 'dim', style: 'grid-column:1/-1;font-size:12px;text-transform:uppercase;letter-spacing:.05em;margin-top:4px' }, t('Advanced')) : null,
    advanced.map(filterBox), active ? h('button', { class: 'small', style: 'align-self:end', onclick: () => { for (const k of Object.keys(st)) if (k.startsWith('f.')) delete st[k]; go(); } }, t('Clear filters')) : null) : null;
  const text = (key, ph, w) => h('input', { placeholder: ph, value: st[key], style: `width:${w}px`, onchange: e => { st[key] = e.target.value.trim(); go(); } });
  set(h('h1', {}, sec.title, ' ', h('span', { class: 'dim', style: 'font-size:16px;font-weight:400' }, t('{n} shown', { n: res.totalSize }))),
    tabs,
    h('div', { class: 'bar-tools' },
      sec.type === 'show' ? sel('type', [['', t('Shows')], ['season', t('Seasons')], ['episode', t('Episodes')]]) : null,
      sel('sort', SORTS),
      sel('genre', [['', t('All genres')], ...genres.map(g => [g.title, `${g.title} (${g.count})`])]),
      sel('watched', [['', t('Watched or not')], ['unwatched', t('Unwatched')], ['watched', t('Watched')], ['progress', t('In progress')]]),
      text('from', t('Year from'), 95), text('to', t('Year to'), 85),
      sel('container', [['', t('Any file type')], ...['mkv', 'mp4', 'avi', 'mov', 'ts', 'm2ts', 'wmv', 'webm', 'mpg'].map(c => [c, c.toUpperCase()])]),
      text('q', t('Filter by title'), 150),
      sizeSlider(),
      h('button', { class: st.more ? 'small primary' : 'small', onclick: () => { st.more = st.more ? '' : '1'; go(); } }, active ? t('More filters ({n})', { n: active }) : t('More filters')),
      h('span', { class: 'grow' }),
      S.me.admin ? h('button', { onclick: async () => { await api('POST', `/library/sections/${id}/refresh`); toast(t('Scanning {name}', { name: sec.title })); pollActivity(); } }, t('Scan now')) : null),
    panel,
    res.Metadata.length ? pickGrid(res.Metadata, { episodeThumb: st.type === 'episode' }) : h('div', { class: 'empty' }, t('Nothing matches these filters.')));
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
    return set(head, tabs, list.length ? h('div', { class: 'cats' }, list.map(d => h('div', { class: 'cat', onclick: open('genre', d.title) },
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
const EXTRA_TYPES = { trailer: 'Trailer', behindthescenes: 'Behind the scenes', deleted: 'Deleted scene', featurette: 'Featurette', interview: 'Interview', scene: 'Scene', short: 'Short', other: 'Extra' };

async function itemPage(id) {
  markNav('');
  const it = meta(await GET('/library/metadata/' + id))[0];
  const m = it.Media && it.Media[0];
  const kids = it.type === 'show' || it.type === 'season' ? meta(await GET(`/library/metadata/${id}/children`)) : [];
  const playlists = meta(await GET('/playlists?kind=manual&owner=me&sort=title'));
  const refresh = () => itemPage(id);
  const watched = it.leafCount ? it.viewedLeafCount === it.leafCount : it.viewCount > 0;
  const crumbs = [];
  if (it.grandparentTitle) crumbs.push(h('a', { href: '#/item/' + it.grandparentRatingKey }, it.grandparentTitle), ' › ');
  if (it.parentTitle) crumbs.push(h('a', { href: '#/item/' + it.parentRatingKey }, seasonTitle(it.parentTitle)));
  const addTo = h('select', { onchange: async e => {
    const v = e.target.value; e.target.value = '';
    if (!v) return;
    if (v === 'new') { const name = await ask(t('Name of the new playlist')); if (name) { await api('POST', '/playlists', { title: name, kind: 'manual', ids: [it.id] }); toast(t('Added to {name}', { name })); } return; }
    const r = await api('POST', `/playlists/${v}/items`, { ids: [it.id] }); toast(r.added ? t('Added to playlist') : t('Already in that playlist'));
  } }, h('option', { value: '' }, t('+ Add to playlist')), playlists.map(p => h('option', { value: p.id }, p.title)), h('option', { value: 'new' }, t('New playlist…')));
  const nextEp = async () => { const l = meta(await GET(`/library/metadata/${id}/allLeaves`)); const e = l.find(x => !x.viewCount) || l[0]; if (e) play(e, e.viewOffset ? e.viewOffset / 1000 : 0, l.slice(l.indexOf(e) + 1)); };
  // Online details (plot, cast, crew …) live in it.extra; seasons and episodes borrow the show's.
  const x = it.extra || {};
  const top = it.grandparentRatingKey || (it.type === 'season' ? it.parentRatingKey : null);
  const topX = top ? ((meta(await GET('/library/metadata/' + top))[0] || {}).extra || {}) : x;
  const cast = x.cast || (it.type !== 'movie' ? topX.cast : null) || [];
  const ids = x.ids || topX.ids || {};
  const crew = [[t('Directed by'), x.directors], [t('Written by'), x.writers], [t('Created by'), x.creators || (it.type !== 'show' ? topX.creators : null)]].filter(r => r[1] && r[1].length);
  const links = [ids.imdb ? ['IMDb', 'https://www.imdb.com/title/' + ids.imdb] : null, ids.tmdb ? ['TMDB', `https://www.themoviedb.org/${it.type === 'movie' ? 'movie' : 'tv'}/${ids.tmdb}`] : null,
    ids.tvmaze ? ['TVmaze', 'https://www.tvmaze.com/shows/' + ids.tvmaze] : null, ids.wikipedia ? ['Wikipedia', ids.wikipedia] : null, ids.wikidata ? ['Wikidata', 'https://www.wikidata.org/wiki/' + ids.wikidata] : null].filter(Boolean);
  const matchable = it.type === 'movie' || it.type === 'show';
  // Rows: Other Editions, Related, More by the director, More with the first two actors.
  const related = matchable ? ((await GET(`/library/metadata/${id}/related?count=20`)).MediaContainer.Hub || []) : [];
  const hero = h('img', { class: 'hero', src: withTok(`/library/metadata/${id}/art?v=${it.updatedAt || 0}`), alt: '', onerror: e => e.target.remove() });
  set(
    hero,
    h('div', { class: 'dim', style: 'margin-bottom:10px;position:relative' }, crumbs),
    h('div', { class: 'detail' },
      h('div', { class: 'poster' }, h('img', { src: withTok(it.thumb), alt: '' })),
      h('div', { class: 'info' },
        h('h1', { style: 'margin-bottom:4px' }, it.type === 'episode' ? `${it.index}. ${it.title}` : it.type === 'season' ? seasonTitle(it.title) : it.title),
        h('div', { class: 'dim' }, [it.year, it.type === 'episode' ? t('Season {n}', { n: it.parentIndex }) : null, fmtDur(it.duration), it.contentRating, it.rating ? `★ ${fmtNum(it.rating)}` : null, it.leafCount ? t('{seen}/{all} watched', { seen: it.viewedLeafCount, all: it.leafCount }) : null].filter(Boolean).join(' · ')),
        scoreBadges(x.ratings || (it.type === 'season' ? topX.ratings : null)),
        it.tagline ? h('p', { style: 'font-style:italic' }, it.tagline) : null,
        it.summary ? h('p', { class: 'plot' }, it.summary) : (S.me.admin && matchable && !x.metadata_source ? h('p', { class: 'dim' }, x.metadata_status === 'unmatched' ? t('No details were found online for this title — use "Fix match" to pick the right one.') : t('Details are being looked up online…')) : null),
        it.genres && it.genres.length ? h('div', {}, it.genres.map(g => h('span', { class: 'tag' }, g))) : null,
        crew.length || it.studio || x.originally_available || x.original_title ? h('div', { class: 'kv credits' },
          crew.map(([k, v]) => [h('div', { class: 'dim' }, k), h('div', {}, v.join(', '))]),
          it.studio ? [h('div', { class: 'dim' }, it.type === 'show' ? t('Network') : t('Studio')), h('div', {}, it.studio)] : null,
          x.original_title && x.original_title !== it.title ? [h('div', { class: 'dim' }, t('Original title')), h('div', {}, x.original_title)] : null,
          x.originally_available ? [h('div', { class: 'dim' }, it.type === 'movie' ? t('Released') : t('First aired')), h('div', {}, new Date(x.originally_available).toLocaleDateString(I18N.lang))] : null) : null,
        links.length ? h('div', { class: 'dim', style: 'margin-top:6px' }, t('More: '), links.map(([n, u], i) => [i ? ' · ' : '', h('a', { href: u, target: '_blank', rel: 'noopener' }, n)])) : null,
        h('div', { class: 'actions' },
          m && it.viewOffset ? h('button', { class: 'primary', onclick: () => play(it, it.viewOffset / 1000) }, t('▶ Resume from {time}', { time: fmtTime(it.viewOffset / 1000) })) : null,
          m ? h('button', { class: it.viewOffset ? '' : 'primary', onclick: () => play(it, 0) }, it.viewOffset ? t('▶ Play from start') : t('▶ Play')) : null,
          !m && it.leafCount ? h('button', { class: 'primary', onclick: nextEp }, t('▶ Play next episode')) : null,
          h('button', { onclick: async () => { await api('POST', `/:/${watched ? 'unscrobble' : 'scrobble'}?key=${it.id}`); refresh(); } }, watched ? t('Mark unwatched') : t('Mark watched')),
          h('button', { onclick: async () => { await api('PUT', `/api/items/${it.id}/favorite`, { favorite: !it.favorite }); refresh(); } }, it.favorite ? t('♥ Favorite') : t('♡ Favorite')),
          starRating(it),
          addTo,
          m ? h('a', { href: withTok(`/library/parts/${it.id}/file?download=1`) }, h('button', {}, t('Download'))) : null,
          S.me.admin ? h('button', { onclick: () => editItem(it) }, t('Edit details')) : null,
          S.me.admin && matchable ? h('button', { onclick: () => fixMatch(it) }, t('Fix match…')) : null,
          S.me.admin && m ? h('button', { onclick: () => creditsDialog(it) }, t('Credits…')) : null,
          S.me.admin && (matchable || top) ? h('button', { onclick: async e => {
            e.target.disabled = true; e.target.textContent = t('Downloading details…');
            try { const r = await api('POST', `/library/metadata/${it.id}/refresh`); toast(t(r.message), 4000); } catch (er) { toast(er.message, 6000); }
            refresh();
          } }, t('Refresh details')) : null),
        m ? h('div', { class: 'panel kv' },
          h('div', {}, t('File type')), h('div', {}, `${m.container.toUpperCase()}${m.formatName ? ' (' + m.formatName + ')' : ''} · ${fmtSize(m.size)}`),
          h('div', {}, t('Video')), h('div', {}, m.videoCodec ? `${m.videoCodec} ${m.width}×${m.height}${m.bitDepth > 8 ? ' ' + t('{n}-bit', { n: m.bitDepth }) : ''}` : t('unknown (ffprobe not available)')),
          h('div', {}, t('Audio')), h('div', {}, m.audioCodec ? t('{codec}, {n} channels', { codec: m.audioCodec, n: m.audioChannels }) : '–'),
          h('div', {}, t('Bitrate')), h('div', {}, m.bitrate ? fmtNum(m.bitrate) + ' kbit/s' : '–'),
          h('div', {}, t('Subtitles')), h('div', {}, (m.subtitles || []).map(s => s.language || s.file || s.codec).join(', ') || t('none')),
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
    kids.length ? [h('h2', {}, it.type === 'show' ? t('Seasons') : t('Episodes')),
      it.type === 'show' ? scroller('row', kids.map(k => posterCard(k))) :
        h('div', { class: 'list' }, kids.map((e, i) => h('div', { class: 'li' },
          h('img', { src: withTok(e.thumb), loading: 'lazy' }),
          h('div', { class: 'grow' }, h('a', { href: '#/item/' + e.id }, h('b', {}, `${e.index}. ${e.title}`)), h('div', { class: 'dim' }, [fmtDur(e.duration), e.viewCount ? t('✓ watched') : '', e.viewOffset ? t('stopped at {time}', { time: fmtTime(e.viewOffset / 1000) }) : ''].filter(Boolean).join(' · ')),
            e.summary ? h('div', { class: 'ep-plot' }, e.summary) : null),
          h('button', { class: 'primary small', onclick: () => play(e, e.viewOffset ? e.viewOffset / 1000 : 0, kids.slice(i + 1)) }, '▶'))))] : null,
    (it.Extras || []).length ? [h('h2', {}, t('Extras')), scroller('row extras', it.Extras.map(x => h('div', { class: 'card', onclick: () => play(x, 0) },
      h('div', { class: 'poster' }, h('img', { src: withTok(x.thumb), loading: 'lazy', alt: '' })),
      h('div', { class: 't', title: t(x.title) }, t(x.title)), h('div', { class: 's' }, [t(EXTRA_TYPES[x.extraType] || 'Extra'), fmtDur(x.duration)].filter(Boolean).join(' · ')))))] : null,
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
  tmdb: () => '<svg viewBox="0 0 70 32" aria-hidden="true"><defs><linearGradient id="tmdb-mark" x1="0" x2="1"><stop offset="0" stop-color="#90cea1"/><stop offset=".56" stop-color="#3cbec9"/><stop offset="1" stop-color="#00b3e5"/></linearGradient></defs><rect width="70" height="32" rx="16" fill="url(#tmdb-mark)"/><text x="35" y="22" text-anchor="middle" font-family="Arial,Helvetica,sans-serif" font-weight="900" font-size="16" fill="#0d253f">TMDB</text></svg>',
};

// Scores from review sites with their marks. Values are as the source gives them.
function scoreBadges(r) {
  if (!r) return null;
  const names = [['imdb', 'IMDb'], ['rotten_tomatoes', 'Rotten Tomatoes'], ['metacritic', 'Metacritic'], ['tmdb', 'TMDB']];
  const shown = names.filter(([k]) => r[k] && r[k].score);
  if (!shown.length) return null;
  return h('div', { class: 'scores' }, shown.map(([k, n]) => {
    const s = r[k];
    const count = s.count ?? s.votes;
    const many = count ? t(s.count_label === 'reviews' ? '{n} reviews' : '{n} votes', { n: Number(count).toLocaleString(I18N.lang) }) : '';
    const mark = h('span', { class: 'mark' });
    mark.innerHTML = SCORE_MARKS[k](parseFloat(String(s.score)) || 0);
    const kids = [mark, h('span', { class: 'num' }, h('b', {}, s.score), h('span', {}, many || n))];
    const title = `${n}: ${s.score}${many ? ' · ' + many : ''}${s.note ? ' — ' + s.note : ''}`;
    return s.url ? h('a', { class: 'score', href: s.url, target: '_blank', rel: 'noopener', title, 'aria-label': title }, kids)
      : h('div', { class: 'score', title, 'aria-label': title }, kids);
  }));
}

// ---------- background work (details downloads, credits detection) in the top bar
const ACT = { done: -1, timer: null, lastRefresh: 0 };
async function pollActivity() {
  clearTimeout(ACT.timer);
  const el = $('#activity');
  if (!S.me || !S.me.admin) { el.style.display = 'none'; return; }
  let st;
  try { st = await GET('/api/metadata/status'); } catch (e) { ACT.timer = setTimeout(pollActivity, 10000); return; }
  const scanning = st.scanning || [];
  const busy = st.queued > 0 || st.working_on || st.job || scanning.length;
  const parts = [];
  if (scanning.length) parts.push(h('span', { class: 'txt' }, t('Scanning {name}', { name: scanning.join(', ') })));
  if (st.job) parts.push(h('span', { class: 'txt' }, t('{label}: {done} of {total}', { label: t(st.job.label), done: st.job.done, total: st.job.total })), h('span', { class: 'pbar' }, h('i', { style: `width:${Math.round(100 * st.job.done / Math.max(1, st.job.total))}%` })));
  if (st.working_on || st.queued > 0) parts.push(h('span', { class: 'txt' }, t('Updating details') + (st.working_title ? ': ' + st.working_title : '') + (st.queued > 0 ? ' · ' + t('{n} left', { n: st.queued }) : '')));
  if (busy) {
    el.replaceChildren(h('span', { class: 'spin' }), ...parts);
    el.title = st.last && st.last.message ? t('Last: {msg}', { msg: t(st.last.message) }) : t('Background work on the server');
    el.style.display = 'flex';
  } else if (ACT.done >= 0 && st.done > ACT.done && st.last && st.last.message) {
    el.replaceChildren(h('span', { class: 'txt' }, (st.last.ok === false ? '⚠ ' : '✓ ') + t(st.last.message)));
    el.style.display = 'flex';
    setTimeout(() => { if (!$('#activity .spin')) el.style.display = 'none'; }, 8000);
  }
  // Show new details as soon as they arrive (unless something is open on top).
  if (ACT.done >= 0 && st.done > ACT.done && !document.querySelector('.modal') && $('#player').style.display !== 'flex') {
    const hash = location.hash.slice(1) || '/';
    const onItem = hash.match(/^\/item\/(\d+)/);
    const fresh = Date.now() - ACT.lastRefresh > 8000;
    if (onItem ? (!st.last || !st.last.id || String(st.last.id) === onItem[1] || !busy) : (/^\/(section\/|$)/.test(hash) && fresh && scrollY < 200)) {
      ACT.lastRefresh = Date.now();
      const y = scrollY; await render(); scrollTo(0, y);
    }
  }
  ACT.done = st.done;
  ACT.timer = setTimeout(pollActivity, busy ? 2500 : 8000);
}

// onclose runs however the box goes away: its buttons, a click beside it or Escape.
function modal(title, kids, onclose) {
  const box = h('div', { class: 'modal', onclick: e => { if (e.target === box) box.remove(); } }, h('div', { class: 'box' }, h('h2', { style: 'margin-top:0' }, title), kids));
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
function fixMatch(it) {
  const x = it.extra || {};
  const title = h('input', { value: x.file_title || it.title, style: 'width:100%' });
  const year = h('input', { value: it.year || '', style: 'width:100px' });
  const results = h('div', { class: 'list', style: 'max-height:55vh;overflow:auto;margin-top:10px' });
  const search = async () => {
    results.replaceChildren(h('div', { class: 'dim' }, t('Searching…')));
    try {
      const r = await GET(`/library/metadata/${it.id}/matches?title=${encodeURIComponent(title.value)}&year=${encodeURIComponent(year.value)}`);
      if (!r.matches.length) return results.replaceChildren(h('div', { class: 'dim' }, t('Nothing found. Try another spelling or leave the year empty.')));
      results.replaceChildren(...r.matches.map(c => h('div', { class: 'li', style: 'cursor:pointer', onclick: async () => {
        results.replaceChildren(h('div', { class: 'dim' }, t('Downloading details for {title}…', { title: c.title })));
        try { const a = await api('PUT', `/library/metadata/${it.id}/match`, { source: c.source, id: c.id }); toast(t(a.message), 4000); box.remove(); itemPage(it.id); }
        catch (e) { results.replaceChildren(h('div', { class: 'err' }, e.message)); }
      } },
        c.thumb ? h('img', { src: c.thumb, referrerpolicy: 'no-referrer' }) : h('div', { class: 'initials', style: 'width:46px;height:68px' }, c.title[0] || '?'),
        h('div', { class: 'grow' }, h('b', {}, c.title), c.year ? ` (${c.year})` : '', h('div', { class: 'dim' }, `${c.source} · ${t('fit {n}%', { n: Math.round(Math.max(0, Math.min(1, c.score)) * 100) })}`),
          c.summary ? h('div', { class: 'ep-plot' }, c.summary) : null))));
    } catch (e) { results.replaceChildren(h('div', { class: 'err' }, e.message)); }
  };
  const box = modal(t('Fix match'), [
    formRow(t('Title'), title), formRow(t('Year'), year),
    h('div', { class: 'actions' }, h('button', { class: 'primary', onclick: search }, t('Search')), h('button', { onclick: () => box.remove() }, t('Close'))),
    results]);
  search();
}

function editItem(it) {
  const f = {};
  const field = (k, label, val, area) => formRow(label, (f[k] = h(area ? 'textarea' : 'input', { value: val ?? '', style: 'width:100%' })), null, area);
  const box = modal(t('Edit details'), [
    field('title', t('Title'), it.title), field('year', t('Year'), it.year), field('summary', t('Summary'), it.summary, true), field('tagline', t('Tagline'), it.tagline),
    field('genres', t('Genres (comma separated)'), (it.genres || []).join(', ')), field('rating', t('Rating (0-10)'), it.rating), field('content_rating', t('Age rating'), it.contentRating), field('studio', t('Studio'), it.studio),
    field('labels', t('Labels (comma separated)'), ((it.extra || {}).labels || []).join(', ')), field('collections', t('Collections (comma separated)'), ((it.extra || {}).collections || []).join(', ')),
    it.type === 'movie' || it.type === 'episode' ? field('edition', t('Edition (e.g. Director’s Cut)'), (it.extra || {}).edition) : null,
    h('div', { class: 'actions' }, h('button', { class: 'primary', onclick: async () => {
      const body = {}; for (const [k, el] of Object.entries(f)) body[k] = el.value;
      body.year = body.year ? Number(body.year) : null; body.rating = body.rating ? Number(String(body.rating).replace(',', '.')) : null;
      await api('PUT', '/library/metadata/' + it.id, body); box.remove(); itemPage(it.id);
    } }, t('Save')), h('button', { onclick: () => box.remove() }, t('Cancel')))]);
}

// ---------- player
const P = { it: null, offset: 0, method: 'direct', timer: null, queue: [], quality: localStorage.getItem('quality') || 'original', audio: '', speed: Number(localStorage.getItem('speed')) || 1 };
const SPEEDS = [0.5, 0.75, 1, 1.25, 1.5, 1.75, 2];
const QUALITIES = [['original', t('Original quality')], ['1080p-20', '1080p 20 Mbps'], ['1080p-12', '1080p 12 Mbps'], ['1080p-8', '1080p 8 Mbps'], ['720p-4', '720p 4 Mbps'], ['720p-3', '720p 3 Mbps'], ['720p-2', '720p 2 Mbps'], ['480p-1.5', '480p ' + fmtNum(1.5) + ' Mbps'], ['320p-0.7', '320p ' + fmtNum(0.7) + ' Mbps']];
const video = $('#video');

async function play(it, startAt, queue) {
  P.queue = queue || [];
  if (!it.Media || !it.Media[0].streams) it = meta(await GET('/library/metadata/' + it.id))[0];
  P.it = it;
  $('#player').style.display = 'flex';
  $('#ptitle').textContent = it.type === 'episode' ? `${it.grandparentTitle} · ${t('S{n}', { n: it.parentIndex })}${t('E{n}', { n: it.index })} ${it.title}` : it.title;
  $('#pquality').replaceChildren(...QUALITIES.map(([v, l]) => h('option', { value: v, selected: v === P.quality }, l)));
  $('#pspeed').replaceChildren(...SPEEDS.map(v => h('option', { value: v, selected: v === P.speed }, v === 1 ? t('Normal speed') : fmtNum(v) + '×')));
  const auds = ((it.Media && it.Media[0].streams) || []).filter(s => s.kind === 'audio');
  $('#paudio').replaceChildren(h('option', { value: '' }, t('Default audio')), ...auds.map(a => h('option', { value: a.index }, `${a.language || t('audio')} ${a.codec} ${a.channels}ch${a.title ? ' ' + a.title : ''}`)));
  $('#paudio').style.display = auds.length > 1 ? '' : 'none';
  P.audio = '';
  // A movie split over files: its parts play one after the other.
  P.parts = ((it.Media && it.Media[0].Part) || []).map(x => (x.duration || 0) / 1000);
  P.part = 0; P.base = 0;
  await load(startAt || 0);
}
// Where part n starts in the whole movie (seconds).
const partStart = n => P.parts.slice(0, n).reduce((a, b) => a + b, 0);
// (Only when every part's length is known; otherwise the parts can't be told apart.)
const split = () => P.parts.length > 1 && P.parts.every(d => d > 0);

async function load(at) {
  const it = P.it, q = P.quality;
  // Which part the position is in; `at` becomes the time inside that part.
  P.part = 0;
  if (split()) while (P.part < P.parts.length - 1 && at >= partStart(P.part + 1)) P.part++;
  P.base = split() ? partStart(P.part) : 0;
  at = Math.max(0, at - P.base);
  const extra = (P.audio ? `&audio=${P.audio}` : '') + (P.part ? `&part=${P.part}` : '');
  const d = await GET(`/library/metadata/${it.id}/decision?quality=${q}${extra}`);
  P.method = P.audio && d.method === 'direct' ? 'remux' : d.method;
  if (!d.allowed) toast(t('Converting is switched off for your account, so this file may not play here.'), 5000);
  video.querySelectorAll('track').forEach(tr => tr.remove());
  if (P.method === 'direct') {
    P.offset = 0;
    video.src = withTok(`/library/parts/${it.id}/file${P.part ? '?part=' + P.part : ''}`);
    video.addEventListener('loadedmetadata', () => { video.currentTime = at; }, { once: true });
  } else {
    P.offset = at;
    video.src = withTok(`/video/transcode/${it.id}/stream.mp4?quality=${q}&offset=${at.toFixed(1)}${extra}`);
  }
  // Subtitles belong to the first file (a later part would show them at the wrong time).
  (P.part ? [] : (it.Media && it.Media[0].subtitles) || []).forEach((s, i) => video.append(h('track', { kind: 'subtitles', src: withTok(s.key), label: s.language || s.file || t('Subtitles {n}', { n: i + 1 }), srclang: (s.language || 'und').slice(0, 2) })));
  $('#pmethod').textContent = { direct: ' · ' + t('direct play'), remux: ' · ' + t('repackaged'), transcode: ' · ' + t('converting') }[P.method] || '';
  $('#pjump').style.display = P.method === 'direct' && !split() ? 'none' : 'flex';
  $('#pseek').max = Math.floor((it.duration || 0) / 1000);
  // A new source starts at the default rate: keep the chosen speed.
  video.defaultPlaybackRate = video.playbackRate = P.speed;
  video.play().catch(() => {});
  clearInterval(P.timer);
  P.timer = setInterval(() => { if (!video.paused) report('playing'); }, 10000);
}
const position = () => (P.base || 0) + P.offset + (video.currentTime || 0);
function report(state) {
  if (!P.it) return;
  api('POST', `/:/timeline?ratingKey=${P.it.id}&time=${Math.floor(position() * 1000)}&state=${state}&client=web`).catch(() => {});
}
video.addEventListener('pause', () => { if (!video.ended) report('paused'); });
video.addEventListener('timeupdate', () => {
  if ((P.method !== 'direct' || split()) && P.it) { $('#ptime').textContent = `${fmtTime(position())} / ${fmtTime((P.it.duration || 0) / 1000)}`; if (!$('#pseek')._drag) $('#pseek').value = Math.floor(position()); }
});
video.addEventListener('ended', () => {
  // The next part of a split movie.
  if (split() && P.part < P.parts.length - 1) { load(partStart(P.part + 1)); return; }
  report('stopped');
  const next = P.queue.shift();
  if (next) play(next, 0, P.queue); else closePlayer(false);
});
$('#pseek').addEventListener('input', e => { e.target._drag = true; $('#ptime').textContent = fmtTime(+e.target.value); });
$('#pseek').addEventListener('change', e => { e.target._drag = false; load(+e.target.value); });
$('#pquality').addEventListener('change', e => { P.quality = e.target.value; localStorage.setItem('quality', P.quality); load(position()); });
$('#paudio').addEventListener('change', e => { P.audio = e.target.value; load(position()); });
function setSpeed(v) {
  P.speed = v;
  try { localStorage.setItem('speed', String(v)); } catch (e) { /* private window */ }
  video.defaultPlaybackRate = video.playbackRate = v;
  $('#pspeed').value = String(v);
}
$('#pspeed').addEventListener('change', e => setSpeed(Number(e.target.value)));
// The browser's own menu can change the speed too: keep the list in step.
video.addEventListener('ratechange', () => { if (P.it && SPEEDS.includes(video.playbackRate) && video.playbackRate !== P.speed) setSpeed(video.playbackRate); });
function closePlayer(send) {
  if (send) report('stopped');
  clearInterval(P.timer);
  video.pause(); video.removeAttribute('src'); video.load();
  $('#player').style.display = 'none';
  P.it = null;
  render();
}
$('#pclose').addEventListener('click', () => closePlayer(true));
document.addEventListener('keydown', e => {
  if (!P.it) return;
  if (e.key === 'Escape') closePlayer(true);
  // < and > step the speed, as in many players.
  if ((e.key === '>' || e.key === '<') && !/^(INPUT|SELECT|TEXTAREA)$/.test(e.target.tagName)) {
    const i = SPEEDS.indexOf(P.speed), next = SPEEDS[Math.max(0, Math.min(SPEEDS.length - 1, (i < 0 ? 2 : i) + (e.key === '>' ? 1 : -1)))];
    setSpeed(next);
    toast(t('Speed: {speed}', { speed: next === 1 ? t('Normal speed') : fmtNum(next) + '×' }));
  }
});
