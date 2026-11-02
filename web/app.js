// Lichtspiel web app, part 1: helpers, sign-in, browsing, details, player.
// Talks only to the server's JSON API (see #/api). Part 2 is pages.js.
'use strict';
// A link with ?token=… (made by the status window and the self-test) signs this browser in once,
// then the token is taken out of the address bar.
(() => {
  const u = new URL(location.href); const t = u.searchParams.get('token');
  if (t) { localStorage.setItem('token', t); u.searchParams.delete('token'); history.replaceState(null, '', u.pathname + u.search + u.hash); }
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
function set(...kids) { main().replaceChildren(...kids.flat(9).filter(Boolean)); window.scrollTo(0, 0); }
function toast(msg, ms = 2600) { const t = $('#toast'); t.textContent = msg; t.style.display = 'block'; clearTimeout(t._t); t._t = setTimeout(() => (t.style.display = 'none'), ms); }
const withTok = p => p + (p.includes('?') ? '&' : '?') + 'token=' + encodeURIComponent(S.token);
const fmtDur = ms => { if (!ms) return ''; const m = Math.round(ms / 60000); if (m < 1) return Math.max(1, Math.round(ms / 1000)) + ' s'; return m >= 60 ? `${Math.floor(m / 60)} h ${m % 60} min` : `${m} min`; };
const fmtTime = s => { s = Math.max(0, Math.floor(s)); const hh = Math.floor(s / 3600), mm = Math.floor(s / 60) % 60, ss = s % 60; return (hh ? hh + ':' + String(mm).padStart(2, '0') : mm) + ':' + String(ss).padStart(2, '0'); };
const fmtDate = t => t ? new Date(t * 1000).toLocaleDateString() : '';
const fmtSize = b => b > 1e9 ? (b / 1e9).toFixed(1) + ' GB' : (b / 1e6).toFixed(0) + ' MB';

async function api(method, path, body) {
  const opt = { method, headers: { 'X-Token': S.token, Accept: 'application/json' } };
  if (body !== undefined) { opt.headers['Content-Type'] = 'application/json'; opt.body = JSON.stringify(body); }
  const r = await fetch(path, opt);
  let j = null;
  try { j = await r.json(); } catch (e) { /* not JSON */ }
  if (r.status === 401 && S.token && !path.startsWith('/api/login')) { logout(); throw new Error('Please sign in again'); }
  if (!r.ok) throw new Error((j && j.error) || 'HTTP ' + r.status);
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
    h('div', { style: 'margin-top:18px' }, h('button', { class: 'primary', type: 'submit', style: 'width:100%' }, button)), err);
  set(form);
  const first = form.querySelector('input'); if (first) first.focus();
}
async function signedIn(token) { S.token = token; localStorage.setItem('token', token); location.hash = '#/'; await boot(); }
function loginPage() {
  authPage('Sign in', S.status ? S.status.name : '', [['username', 'User name'], ['password', 'Password', 'password']], 'Sign in',
    async v => { const r = await api('POST', '/api/login', { username: v.username, password: v.password, device: 'web' }); await signedIn(r.token); });
}
function setupPage() {
  authPage('Welcome', 'Create the owner account for this server. You can invite other people afterwards.',
    [['server_name', 'Server name', 'text', 'Lichtspiel'], ['username', 'Your user name'], ['password', 'Password (at least 6 characters)', 'password']], 'Create account',
    async v => { const r = await api('POST', '/api/setup', v); await signedIn(r.token); });
}
async function invitePage(code) {
  let info;
  try { info = await api('GET', '/api/invites/' + code); } catch (e) { return set(h('div', { class: 'center' }, h('h1', {}, 'Invite'), h('p', { class: 'err' }, e.message), h('a', { href: '#/login' }, 'Sign in instead'))); }
  authPage('Join ' + info.server, `${info.invited_by || 'Someone'} invited you${info.note ? ': ' + info.note : ''}. Pick a user name and password.`,
    [['username', 'User name'], ['email', 'Email (optional)', 'email', info.email], ['password', 'Password (at least 6 characters)', 'password']], 'Join',
    async v => { const r = await api('POST', `/api/invites/${code}/accept`, v); await signedIn(r.token); });
}

// ---------- chrome
async function boot() {
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
  who.append(h('span', {}, S.me.username + (S.me.admin ? ' (admin)' : '')), h('button', { class: 'small', onclick: logout }, 'Sign out'));
  document.querySelectorAll('#side .admin').forEach(e => (e.style.display = S.me.admin ? '' : 'none'));
  $('#navsections').replaceChildren(...S.sections.map(s => h('a', { href: '#/section/' + s.id, 'data-nav': 'section/' + s.id },
    h('span', {}, s.title), h('small', {}, s.type === 'show' ? s.count + ' shows' : s.videoCount))));
}
function markNav(key) { document.querySelectorAll('#side a').forEach(a => a.classList.toggle('on', a.dataset.nav === key)); }
let searchTimer;
$('#q').addEventListener('input', e => { clearTimeout(searchTimer); searchTimer = setTimeout(() => { const v = e.target.value.trim(); location.hash = v ? '#/search/' + encodeURIComponent(v) : '#/'; }, 300); });

// ---------- cards
function posterCard(it, opts = {}) {
  const prog = it.viewOffset && it.duration ? Math.min(1, it.viewOffset / it.duration) : 0;
  let title = it.title, sub = it.year || '';
  if (it.type === 'episode') { title = it.grandparentTitle || it.title; sub = `S${it.parentIndex || 0} E${it.index || 0} · ${it.title}`; }
  if (it.type === 'season') sub = it.parentTitle || '';
  if (it.type === 'show') sub = (it.leafCount || 0) + ' episodes';
  const unseen = it.leafCount ? it.leafCount - (it.viewedLeafCount || 0) : 0;
  const img = it.type === 'episode' && it.grandparentThumb && !opts.episodeThumb ? it.grandparentThumb : it.thumb;
  return h('div', { class: 'card', onclick: () => (location.hash = '#/item/' + it.id) },
    h('div', { class: 'poster' }, h('img', { src: withTok(img), loading: 'lazy', alt: '' }),
      prog > 0 ? h('div', { class: 'bar' }, h('i', { style: `width:${prog * 100}%` })) : null,
      it.leafCount && unseen > 0 ? h('span', { class: 'badge' }, unseen) : null,
      (it.viewCount > 0 || (it.leafCount && unseen === 0)) ? h('span', { class: 'seen', title: 'Watched' }) : null),
    h('div', { class: 't', title }, title), h('div', { class: 's' }, sub));
}
// A sideways row without a scrollbar: arrows on the left and right (shown on
// hover when there is more to see) scroll it by most of its width.
function scroller(cls, kids) {
  const row = h('div', { class: cls }, kids);
  const go = d => row.scrollBy({ left: d * row.clientWidth * 0.85 });
  const l = h('button', { class: 'arrow l', title: 'Back', onclick: () => go(-1) }, '‹');
  const r = h('button', { class: 'arrow r', title: 'More', onclick: () => go(1) }, '›');
  const upd = () => { l.classList.toggle('show', row.scrollLeft > 4); r.classList.toggle('show', row.scrollLeft + row.clientWidth < row.scrollWidth - 4); };
  row.addEventListener('scroll', upd, { passive: true });
  new ResizeObserver(upd).observe(row);
  return h('div', { class: 'rowwrap' }, l, row, r);
}
const row = (title, items) => items.length ? [h('h2', {}, title), scroller('row', items.map(i => posterCard(i)))] : null;

// ---------- home, search, browse
async function homePage() {
  markNav('home');
  const hubs = (await GET('/hubs')).MediaContainer.Hub;
  if (!S.sections.length) {
    return set(h('h1', {}, 'Welcome'), h('div', { class: 'empty' },
      S.me.admin ? [h('p', {}, 'No libraries yet. Add your movie and TV show folders to get started.'), h('button', { class: 'primary', onclick: () => (location.hash = '#/libraries') }, 'Add a library')]
        : 'The owner has not shared any libraries with you yet.'));
  }
  set(h('h1', {}, 'Home'), hubs.length ? hubs.map(hb => row(hb.title, hb.Metadata)) : h('div', { class: 'empty' }, 'Nothing here yet. Libraries are still being scanned, or they are empty.'));
}
async function searchPage(q) {
  markNav('');
  const r = meta(await GET('/hubs/search?query=' + encodeURIComponent(q)));
  set(h('h1', {}, `Search: ${q}`), r.length ? h('div', { class: 'grid' }, r.map(i => posterCard(i))) : h('div', { class: 'empty' }, 'Nothing found.'));
}

const SORTS = [['title', 'Title'], ['year:desc', 'Year (newest)'], ['year', 'Year (oldest)'], ['added_at:desc', 'Recently added'], ['rating:desc', 'Rating'], ['duration:desc', 'Length'], ['last_viewed_at:desc', 'Recently watched'], ['view_count:desc', 'Most watched'], ['random', 'Random']];

async function sectionPage(id, qs) {
  markNav('section/' + id);
  const sec = S.sections.find(s => String(s.id) === String(id));
  if (!sec) return set(h('div', { class: 'empty' }, 'Library not found.'));
  const p = new URLSearchParams(qs || '');
  const st = {}; for (const k of ['type', 'sort', 'genre', 'watched', 'from', 'to', 'q', 'container']) st[k] = p.get(k) || '';
  if (!st.sort) st.sort = 'title';
  const go = () => { const n = new URLSearchParams(); for (const [k, v] of Object.entries(st)) if (v) n.set(k, v); location.hash = `#/section/${id}?${n}`; };
  const genres = (await GET(`/library/sections/${id}/genres`)).MediaContainer.Directory;
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
  const res = (await GET(`/library/sections/${id}/all?${q}`)).MediaContainer;
  const sel = (key, opts) => h('select', { onchange: e => { st[key] = e.target.value; go(); } }, opts.map(([v, l]) => h('option', { value: v, selected: st[key] === v }, l)));
  const text = (key, ph, w) => h('input', { placeholder: ph, value: st[key], style: `width:${w}px`, onchange: e => { st[key] = e.target.value.trim(); go(); } });
  set(h('h1', {}, sec.title, ' ', h('span', { class: 'dim', style: 'font-size:16px;font-weight:400' }, res.totalSize + ' shown')),
    h('div', { class: 'bar-tools' },
      sec.type === 'show' ? sel('type', [['', 'Shows'], ['season', 'Seasons'], ['episode', 'Episodes']]) : null,
      sel('sort', SORTS),
      sel('genre', [['', 'All genres'], ...genres.map(g => [g.title, `${g.title} (${g.count})`])]),
      sel('watched', [['', 'Watched or not'], ['unwatched', 'Unwatched'], ['watched', 'Watched'], ['progress', 'In progress']]),
      text('from', 'Year from', 95), text('to', 'Year to', 85),
      sel('container', [['', 'Any file type'], ...['mkv', 'mp4', 'avi', 'mov', 'ts', 'm2ts', 'wmv', 'webm', 'mpg'].map(c => [c, c.toUpperCase()])]),
      text('q', 'Filter by title', 150),
      h('span', { class: 'grow' }),
      S.me.admin ? h('button', { onclick: async () => { await api('POST', `/library/sections/${id}/refresh`); toast('Scanning ' + sec.title); } }, 'Scan now') : null),
    res.Metadata.length ? h('div', { class: 'grid' }, res.Metadata.map(i => posterCard(i, { episodeThumb: st.type === 'episode' }))) : h('div', { class: 'empty' }, 'Nothing matches these filters.'));
}

// ---------- details
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
  if (it.parentTitle) crumbs.push(h('a', { href: '#/item/' + it.parentRatingKey }, it.parentTitle));
  const addTo = h('select', { onchange: async e => {
    const v = e.target.value; e.target.value = '';
    if (!v) return;
    if (v === 'new') { const t = prompt('Name of the new playlist'); if (t) { await api('POST', '/playlists', { title: t, kind: 'manual', ids: [it.id] }); toast('Added to ' + t); } return; }
    const r = await api('POST', `/playlists/${v}/items`, { ids: [it.id] }); toast(r.added ? 'Added to playlist' : 'Already in that playlist');
  } }, h('option', { value: '' }, '+ Add to playlist'), playlists.map(p => h('option', { value: p.id }, p.title)), h('option', { value: 'new' }, 'New playlist…'));
  const nextEp = async () => { const l = meta(await GET(`/library/metadata/${id}/allLeaves`)); const e = l.find(x => !x.viewCount) || l[0]; if (e) play(e, e.viewOffset ? e.viewOffset / 1000 : 0, l.slice(l.indexOf(e) + 1)); };
  // Online details (plot, cast, crew …) live in it.extra; seasons and episodes borrow the show's.
  const x = it.extra || {};
  const top = it.grandparentRatingKey || (it.type === 'season' ? it.parentRatingKey : null);
  const topX = top ? ((meta(await GET('/library/metadata/' + top))[0] || {}).extra || {}) : x;
  const cast = x.cast || (it.type !== 'movie' ? topX.cast : null) || [];
  const ids = x.ids || topX.ids || {};
  const crew = [['Directed by', x.directors], ['Written by', x.writers], ['Created by', x.creators || (it.type !== 'show' ? topX.creators : null)]].filter(r => r[1] && r[1].length);
  const links = [ids.imdb ? ['IMDb', 'https://www.imdb.com/title/' + ids.imdb] : null, ids.tmdb ? ['TMDB', `https://www.themoviedb.org/${it.type === 'movie' ? 'movie' : 'tv'}/${ids.tmdb}`] : null,
    ids.tvmaze ? ['TVmaze', 'https://www.tvmaze.com/shows/' + ids.tvmaze] : null, ids.wikipedia ? ['Wikipedia', ids.wikipedia] : null, ids.wikidata ? ['Wikidata', 'https://www.wikidata.org/wiki/' + ids.wikidata] : null].filter(Boolean);
  const matchable = it.type === 'movie' || it.type === 'show';
  const hero = h('img', { class: 'hero', src: withTok(`/library/metadata/${id}/art?v=${it.updatedAt || 0}`), alt: '', onerror: e => e.target.remove() });
  set(
    hero,
    h('div', { class: 'dim', style: 'margin-bottom:10px;position:relative' }, crumbs),
    h('div', { class: 'detail' },
      h('div', { class: 'poster' }, h('img', { src: withTok(it.thumb), alt: '' })),
      h('div', { class: 'info' },
        h('h1', { style: 'margin-bottom:4px' }, it.type === 'episode' ? `${it.index}. ${it.title}` : it.title),
        h('div', { class: 'dim' }, [it.year, it.type === 'episode' ? `Season ${it.parentIndex}` : null, fmtDur(it.duration), it.contentRating, it.rating ? `★ ${it.rating}` : null, it.leafCount ? `${it.viewedLeafCount}/${it.leafCount} watched` : null].filter(Boolean).join(' · ')),
        scoreBadges(x.ratings || (it.type === 'season' ? topX.ratings : null)),
        it.tagline ? h('p', { style: 'font-style:italic' }, it.tagline) : null,
        it.summary ? h('p', { class: 'plot' }, it.summary) : (S.me.admin && matchable && !x.metadata_source ? h('p', { class: 'dim' }, x.metadata_status === 'unmatched' ? 'No details were found online for this title — use "Fix match" to pick the right one.' : 'Details are being looked up online…') : null),
        it.genres && it.genres.length ? h('div', {}, it.genres.map(g => h('span', { class: 'tag' }, g))) : null,
        crew.length || it.studio || x.originally_available || x.original_title ? h('div', { class: 'kv credits' },
          crew.map(([k, v]) => [h('div', { class: 'dim' }, k), h('div', {}, v.join(', '))]),
          it.studio ? [h('div', { class: 'dim' }, it.type === 'show' ? 'Network' : 'Studio'), h('div', {}, it.studio)] : null,
          x.original_title && x.original_title !== it.title ? [h('div', { class: 'dim' }, 'Original title'), h('div', {}, x.original_title)] : null,
          x.originally_available ? [h('div', { class: 'dim' }, it.type === 'movie' ? 'Released' : 'First aired'), h('div', {}, new Date(x.originally_available).toLocaleDateString())] : null) : null,
        links.length ? h('div', { class: 'dim', style: 'margin-top:6px' }, 'More: ', links.map(([n, u], i) => [i ? ' · ' : '', h('a', { href: u, target: '_blank', rel: 'noopener' }, n)])) : null,
        h('div', { class: 'actions' },
          m && it.viewOffset ? h('button', { class: 'primary', onclick: () => play(it, it.viewOffset / 1000) }, '▶ Resume from ' + fmtTime(it.viewOffset / 1000)) : null,
          m ? h('button', { class: it.viewOffset ? '' : 'primary', onclick: () => play(it, 0) }, '▶ Play' + (it.viewOffset ? ' from start' : '')) : null,
          !m && it.leafCount ? h('button', { class: 'primary', onclick: nextEp }, '▶ Play next episode') : null,
          h('button', { onclick: async () => { await api('POST', `/:/${watched ? 'unscrobble' : 'scrobble'}?key=${it.id}`); refresh(); } }, watched ? 'Mark unwatched' : 'Mark watched'),
          h('button', { onclick: async () => { await api('PUT', `/api/items/${it.id}/favorite`, { favorite: !it.favorite }); refresh(); } }, it.favorite ? '♥ Favorite' : '♡ Favorite'),
          starRating(it),
          addTo,
          m ? h('a', { href: withTok(`/library/parts/${it.id}/file?download=1`) }, h('button', {}, 'Download')) : null,
          S.me.admin ? h('button', { onclick: () => editItem(it) }, 'Edit details') : null,
          S.me.admin && matchable ? h('button', { onclick: () => fixMatch(it) }, 'Fix match…') : null,
          S.me.admin && m ? h('button', { onclick: () => creditsDialog(it) }, 'Credits…') : null,
          S.me.admin && (matchable || top) ? h('button', { onclick: async e => {
            e.target.disabled = true; e.target.textContent = 'Downloading details…';
            try { const r = await api('POST', `/library/metadata/${it.id}/refresh`); toast(r.message, 4000); } catch (er) { toast(er.message, 6000); }
            refresh();
          } }, 'Refresh details') : null),
        m ? h('div', { class: 'panel kv' },
          h('div', {}, 'File type'), h('div', {}, `${m.container.toUpperCase()}${m.formatName ? ' (' + m.formatName + ')' : ''} · ${fmtSize(m.size)}`),
          h('div', {}, 'Video'), h('div', {}, m.videoCodec ? `${m.videoCodec} ${m.width}×${m.height}${m.bitDepth > 8 ? ' ' + m.bitDepth + '-bit' : ''}` : 'unknown (ffprobe not available)'),
          h('div', {}, 'Audio'), h('div', {}, m.audioCodec ? `${m.audioCodec}, ${m.audioChannels} channels` : '–'),
          h('div', {}, 'Bitrate'), h('div', {}, m.bitrate ? m.bitrate + ' kbit/s' : '–'),
          h('div', {}, 'Subtitles'), h('div', {}, (m.subtitles || []).map(s => s.language || s.file || s.codec).join(', ') || 'none'),
          m.file ? [h('div', {}, 'Path'), h('div', { style: 'word-break:break-all' }, m.file)] : null,
          h('div', {}, 'In your browser'), h('div', { id: 'decision', class: 'dim' }, '…')) : null)),
    cast.length ? [h('h2', {}, 'Cast'), scroller('row cast', cast.map(p => {
      const initials = () => h('div', { class: 'initials' }, (p.name || '?').split(' ').map(w => w[0]).slice(0, 2).join(''));
      // The photo saved on this server; the original web address only as a fallback.
      const src = p.thumb ? withTok(p.thumb) : p.photo;
      const pic = src ? h('img', { src, alt: '', loading: 'lazy', referrerpolicy: 'no-referrer', onerror: e => e.target.replaceWith(initials()) }) : initials();
      // Actors are shared: the name opens a page with everything they are in.
      return h('a', { class: 'person', href: `#/person/${encodeURIComponent(p.key || 'byname')}?name=${encodeURIComponent(p.name || '')}` }, pic, h('b', {}, p.name), p.role ? h('div', { class: 'dim' }, p.role) : null,
        p.count > 1 ? h('div', { class: 'dim' }, `${p.count} in this library`) : null);
    }))] : null,
    kids.length ? [h('h2', {}, it.type === 'show' ? 'Seasons' : 'Episodes'),
      it.type === 'show' ? scroller('row', kids.map(k => posterCard(k))) :
        h('div', { class: 'list' }, kids.map((e, i) => h('div', { class: 'li' },
          h('img', { src: withTok(e.thumb), loading: 'lazy' }),
          h('div', { class: 'grow' }, h('a', { href: '#/item/' + e.id }, h('b', {}, `${e.index}. ${e.title}`)), h('div', { class: 'dim' }, [fmtDur(e.duration), e.viewCount ? '✓ watched' : '', e.viewOffset ? 'stopped at ' + fmtTime(e.viewOffset / 1000) : ''].filter(Boolean).join(' · ')),
            e.summary ? h('div', { class: 'ep-plot' }, e.summary) : null),
          h('button', { class: 'primary small', onclick: () => play(e, e.viewOffset ? e.viewOffset / 1000 : 0, kids.slice(i + 1)) }, '▶'))))] : null);
  if (m) {
    try {
      const d = await GET(`/library/metadata/${id}/decision`);
      const words = { direct: 'Plays the original file directly', remux: 'Repackaged on the fly (picture untouched)', transcode: 'Converted on the fly' };
      $('#decision').textContent = words[d.method] + (d.reasons.length ? ' — ' + d.reasons.join('; ') : '');
    } catch (e) { /* ignore */ }
  }
}

// Your own rating: five stars, each worth 2 on the server's 0-10 scale.
function starRating(it) {
  let cur = it.userRating || 0;
  const box = h('span', { class: 'stars', title: 'Your rating' });
  const clear = h('button', { class: 'clear', title: 'Remove your rating' }, 'clear');
  // Half stars: each point of the 0–10 scale is half a star; the left half of a
  // star gives the half, the right half the whole star.
  const paint = (v = cur) => {
    const pts = Math.round(v);
    [...box.children].forEach((b, i) => { b.classList.toggle('on', (i + 1) * 2 <= pts); b.classList.toggle('half', (i + 1) * 2 - 1 === pts); });
    clear.style.visibility = cur ? '' : 'hidden';
  };
  const stars = v => (v / 2).toLocaleString(undefined, { maximumFractionDigits: 1 });
  const pointsAt = (e, i) => { const r = e.currentTarget.getBoundingClientRect(); return i * 2 - (e.clientX - r.left < r.width / 2 ? 1 : 0); };
  const save = async v => {
    await api('POST', `/:/rate?key=${it.id}&rating=${v}`);
    cur = v < 0 ? 0 : v; it.userRating = cur; paint();
    toast(v < 0 ? 'Rating removed' : `Rated ${stars(v)} of 5 stars`);
  };
  for (let i = 1; i <= 5; i++) box.append(h('button', { 'aria-label': `${i} stars (click the left half for ${i - 0.5})`, title: `${i - 0.5} or ${i} of 5`,
    onmousemove: e => paint(pointsAt(e, i)), onclick: e => save(e.detail === 0 ? i * 2 : pointsAt(e, i)) }, '★'));
  box.onmouseleave = () => paint();
  clear.onclick = () => save(-1);
  paint();
  return h('span', { style: 'display:inline-flex;align-items:center' }, box, clear);
}

// Scores from review sites (text only, no logos). Values are as the source gives them.
function scoreBadges(r) {
  if (!r) return null;
  const names = [['imdb', 'IMDb'], ['rotten_tomatoes', 'Rotten Tomatoes'], ['metacritic', 'Metacritic'], ['tmdb', 'TMDB']];
  const shown = names.filter(([k]) => r[k] && r[k].score);
  if (!shown.length) return null;
  return h('div', { class: 'scores' }, shown.map(([k, n]) => {
    const s = r[k];
    const kids = [h('b', {}, s.score), h('span', {}, n + (s.votes ? ` · ${Number(s.votes).toLocaleString()} votes` : ''))];
    return s.url ? h('a', { class: 'score', href: s.url, target: '_blank', rel: 'noopener', title: s.note || n }, kids) : h('div', { class: 'score', title: s.note || n }, kids);
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
  const busy = st.queued > 0 || st.working_on || st.job;
  const parts = [];
  if (st.job) parts.push(h('span', { class: 'txt' }, `${st.job.label}: ${st.job.done} of ${st.job.total}`), h('span', { class: 'pbar' }, h('i', { style: `width:${Math.round(100 * st.job.done / Math.max(1, st.job.total))}%` })));
  if (st.working_on || st.queued > 0) parts.push(h('span', { class: 'txt' }, `Updating details${st.working_title ? ': ' + st.working_title : ''}${st.queued > 0 ? ` · ${st.queued} left` : ''}`));
  if (busy) {
    el.replaceChildren(h('span', { class: 'spin' }), ...parts);
    el.title = st.last && st.last.message ? 'Last: ' + st.last.message : 'Background work on the server';
    el.style.display = 'flex';
  } else if (ACT.done >= 0 && st.done > ACT.done && st.last && st.last.message) {
    el.replaceChildren(h('span', { class: 'txt' }, (st.last.ok === false ? '⚠ ' : '✓ ') + st.last.message));
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

function modal(title, kids) {
  const box = h('div', { class: 'modal', onclick: e => { if (e.target === box) box.remove(); } }, h('div', { class: 'box' }, h('h2', { style: 'margin-top:0' }, title), kids));
  document.body.append(box);
  return box;
}

// Where the end credits start: decides when the video counts as watched.
async function creditsDialog(it) {
  const r = await GET(`/library/metadata/${it.id}/markers`);
  const parse = t => { const p = t.trim().split(':').map(Number); if (!t.trim() || p.some(isNaN)) return null; return p.reduce((a, b) => a * 60 + b, 0) * 1000; };
  const inp = h('input', { value: r.credits.length ? fmtTime(r.credits[0] / 1000) : '', placeholder: 'e.g. 1:52:30', style: 'width:140px' });
  const from = { manual: 'set by hand', chapters: 'from a chapter named Credits in the file', picture: 'found in the picture (dark screen with text near the end)', none: 'none found (no Credits chapter, and the end of the video doesn’t look like credits)', pending: 'not looked for yet (detection runs in the background)' }[r.source] + (r.errors ? `; detection failed ${r.errors} time(s)` : '');
  const save = async credits => { try { await api('PUT', `/library/metadata/${it.id}/markers`, { credits }); toast('Saved'); box.remove(); } catch (e) { toast(e.message, 5000); } };
  const box = modal('End credits', [
    h('p', { class: 'dim' }, `Credits marker: ${from}. This video counts as watched at ${fmtTime(r.watchedAt / 1000)} of ${fmtTime(r.duration / 1000)}.`),
    h('label', {}, 'Credits start at (hours:minutes:seconds)'), inp,
    h('div', { class: 'actions' },
      h('button', { class: 'primary', onclick: () => { const ms = parse(inp.value); if (ms === null) return toast('Enter a time like 1:52:30'); save([ms]); } }, 'Save'),
      r.source === 'manual' ? h('button', { onclick: () => save(null) }, 'Use the file’s chapters') : null,
      h('button', { title: 'Read the chapters of this file again and look for the end credits', onclick: async () => {
        try { await api('POST', `/library/metadata/${it.id}/credits`); toast('Detecting credits…'); box.remove(); pollActivity(); setTimeout(() => creditsDialog(it), 2500); } catch (e) { toast(e.message, 6000); }
      } }, 'Detect credits again'),
      h('button', { onclick: () => box.remove() }, 'Cancel'))]);
}

// Pick the right online entry when the automatic match was wrong or missing.
function fixMatch(it) {
  const x = it.extra || {};
  const title = h('input', { value: x.file_title || it.title, style: 'width:100%' });
  const year = h('input', { value: it.year || '', style: 'width:100px' });
  const results = h('div', { class: 'list', style: 'max-height:55vh;overflow:auto;margin-top:10px' });
  const search = async () => {
    results.replaceChildren(h('div', { class: 'dim' }, 'Searching…'));
    try {
      const r = await GET(`/library/metadata/${it.id}/matches?title=${encodeURIComponent(title.value)}&year=${encodeURIComponent(year.value)}`);
      if (!r.matches.length) return results.replaceChildren(h('div', { class: 'dim' }, 'Nothing found. Try another spelling or leave the year empty.'));
      results.replaceChildren(...r.matches.map(c => h('div', { class: 'li', style: 'cursor:pointer', onclick: async () => {
        results.replaceChildren(h('div', { class: 'dim' }, 'Downloading details for ' + c.title + '…'));
        try { const a = await api('PUT', `/library/metadata/${it.id}/match`, { source: c.source, id: c.id }); toast(a.message, 4000); box.remove(); itemPage(it.id); }
        catch (e) { results.replaceChildren(h('div', { class: 'err' }, e.message)); }
      } },
        c.thumb ? h('img', { src: c.thumb, referrerpolicy: 'no-referrer' }) : h('div', { class: 'initials', style: 'width:46px;height:68px' }, c.title[0] || '?'),
        h('div', { class: 'grow' }, h('b', {}, c.title), c.year ? ` (${c.year})` : '', h('div', { class: 'dim' }, `${c.source} · fit ${Math.round(Math.max(0, Math.min(1, c.score)) * 100)}%`),
          c.summary ? h('div', { class: 'ep-plot' }, c.summary) : null))));
    } catch (e) { results.replaceChildren(h('div', { class: 'err' }, e.message)); }
  };
  const box = modal('Fix match', [
    h('label', {}, 'Title'), title, h('label', {}, 'Year'), year,
    h('div', { class: 'actions' }, h('button', { class: 'primary', onclick: search }, 'Search'), h('button', { onclick: () => box.remove() }, 'Close')),
    results]);
  search();
}

function editItem(it) {
  const f = {};
  const field = (k, label, val, area) => [h('label', {}, label), (f[k] = h(area ? 'textarea' : 'input', { value: val ?? '', style: 'width:100%' }))];
  const box = modal('Edit details', [
    field('title', 'Title', it.title), field('year', 'Year', it.year), field('summary', 'Summary', it.summary, true), field('tagline', 'Tagline', it.tagline),
    field('genres', 'Genres (comma separated)', (it.genres || []).join(', ')), field('rating', 'Rating (0-10)', it.rating), field('content_rating', 'Age rating', it.contentRating), field('studio', 'Studio', it.studio),
    h('div', { class: 'actions' }, h('button', { class: 'primary', onclick: async () => {
      const body = {}; for (const [k, el] of Object.entries(f)) body[k] = el.value;
      body.year = body.year ? Number(body.year) : null; body.rating = body.rating ? Number(body.rating) : null;
      await api('PUT', '/library/metadata/' + it.id, body); box.remove(); itemPage(it.id);
    } }, 'Save'), h('button', { onclick: () => box.remove() }, 'Cancel'))]);
}

// ---------- player
const P = { it: null, offset: 0, method: 'direct', timer: null, queue: [], quality: localStorage.getItem('quality') || 'original', audio: '' };
const QUALITIES = [['original', 'Original quality'], ['1080p-20', '1080p 20 Mbps'], ['1080p-12', '1080p 12 Mbps'], ['1080p-8', '1080p 8 Mbps'], ['720p-4', '720p 4 Mbps'], ['720p-3', '720p 3 Mbps'], ['720p-2', '720p 2 Mbps'], ['480p-1.5', '480p 1.5 Mbps'], ['320p-0.7', '320p 0.7 Mbps']];
const video = $('#video');

async function play(it, startAt, queue) {
  P.queue = queue || [];
  if (!it.Media || !it.Media[0].streams) it = meta(await GET('/library/metadata/' + it.id))[0];
  P.it = it;
  $('#player').style.display = 'flex';
  $('#ptitle').textContent = it.type === 'episode' ? `${it.grandparentTitle} · S${it.parentIndex}E${it.index} ${it.title}` : it.title;
  $('#pquality').replaceChildren(...QUALITIES.map(([v, l]) => h('option', { value: v, selected: v === P.quality }, l)));
  const auds = ((it.Media && it.Media[0].streams) || []).filter(s => s.kind === 'audio');
  $('#paudio').replaceChildren(h('option', { value: '' }, 'Default audio'), ...auds.map(a => h('option', { value: a.index }, `${a.language || 'audio'} ${a.codec} ${a.channels}ch${a.title ? ' ' + a.title : ''}`)));
  $('#paudio').style.display = auds.length > 1 ? '' : 'none';
  P.audio = '';
  await load(startAt || 0);
}

async function load(at) {
  const it = P.it, q = P.quality;
  const extra = P.audio ? `&audio=${P.audio}` : '';
  const d = await GET(`/library/metadata/${it.id}/decision?quality=${q}${extra}`);
  P.method = P.audio && d.method === 'direct' ? 'remux' : d.method;
  if (!d.allowed) toast('Converting is switched off for your account, so this file may not play here.', 5000);
  video.querySelectorAll('track').forEach(t => t.remove());
  if (P.method === 'direct') {
    P.offset = 0;
    video.src = withTok(`/library/parts/${it.id}/file`);
    video.addEventListener('loadedmetadata', () => { video.currentTime = at; }, { once: true });
  } else {
    P.offset = at;
    video.src = withTok(`/video/transcode/${it.id}/stream.mp4?quality=${q}&offset=${at.toFixed(1)}${extra}`);
  }
  ((it.Media && it.Media[0].subtitles) || []).forEach((s, i) => video.append(h('track', { kind: 'subtitles', src: withTok(s.key), label: s.language || s.file || 'Subtitles ' + (i + 1), srclang: (s.language || 'und').slice(0, 2) })));
  $('#pmethod').textContent = { direct: ' · direct play', remux: ' · repackaged', transcode: ' · converting' }[P.method] || '';
  $('#pjump').style.display = P.method === 'direct' ? 'none' : 'flex';
  $('#pseek').max = Math.floor((it.duration || 0) / 1000);
  video.play().catch(() => {});
  clearInterval(P.timer);
  P.timer = setInterval(() => { if (!video.paused) report('playing'); }, 10000);
}
const position = () => P.offset + (video.currentTime || 0);
function report(state) {
  if (!P.it) return;
  api('POST', `/:/timeline?ratingKey=${P.it.id}&time=${Math.floor(position() * 1000)}&state=${state}&client=web`).catch(() => {});
}
video.addEventListener('pause', () => { if (!video.ended) report('paused'); });
video.addEventListener('timeupdate', () => {
  if (P.method !== 'direct' && P.it) { $('#ptime').textContent = `${fmtTime(position())} / ${fmtTime((P.it.duration || 0) / 1000)}`; if (!$('#pseek')._drag) $('#pseek').value = Math.floor(position()); }
});
video.addEventListener('ended', () => {
  report('stopped');
  const next = P.queue.shift();
  if (next) play(next, 0, P.queue); else closePlayer(false);
});
$('#pseek').addEventListener('input', e => { e.target._drag = true; $('#ptime').textContent = fmtTime(+e.target.value); });
$('#pseek').addEventListener('change', e => { e.target._drag = false; load(+e.target.value); });
$('#pquality').addEventListener('change', e => { P.quality = e.target.value; localStorage.setItem('quality', P.quality); load(position()); });
$('#paudio').addEventListener('change', e => { P.audio = e.target.value; load(position()); });
function closePlayer(send) {
  if (send) report('stopped');
  clearInterval(P.timer);
  video.pause(); video.removeAttribute('src'); video.load();
  $('#player').style.display = 'none';
  P.it = null;
  render();
}
$('#pclose').addEventListener('click', () => closePlayer(true));
document.addEventListener('keydown', e => { if (e.key === 'Escape' && P.it) closePlayer(true); });
