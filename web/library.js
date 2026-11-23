// Lichtspiel web app: a library's page (filters, sorting, the letters at the side, posters that load
// while you scroll) and its tabs Recommended, Collections, Playlists and Categories. Loaded after app.js.
'use strict';

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
// The lists of filter values cover the whole library whatever is filtered, so they are fetched once a
// minute at most (and ahead of the click) instead of on every page and every change of a filter.
const FILTER_CACHE = new Map();
function cachedFilters(url) {
  const hit = FILTER_CACHE.get(url);
  if (hit && Date.now() - hit.at < 60000) return hit.p;
  const p = GET(url).then(r => r.MediaContainer.Filter);
  FILTER_CACHE.set(url, { at: Date.now(), p });
  p.catch(() => FILTER_CACHE.delete(url));
  return p;
}

/** @param {{ st: any, go: any, advRules: any, lastKey: string, loadFilters: any, prefetch?: any }} o */
function filterUI({ st, go, advRules, lastKey, loadFilters, prefetch }) {
  // Ask for the values shortly after the page is up, so the panel opens at once.
  if (prefetch) setTimeout(() => { Promise.resolve().then(loadFilters).catch(() => {}); }, 400);
  const filterName = f => t(FILTER_NAMES[f] || f);
  const valueName = (f, v) => f === 'video_resolution' ? (RES_NAMES[v] || v) : f === 'decade' ? t('{v}s', { v }) : /_language$/.test(f) ? (langName(v) || v) : v;
  const setFilter = (f, v) => { if (v) st['f.' + f] = v; else delete st['f.' + f]; go(); };
  // ----- the filter panel: the kinds of filter on the left, the values of the chosen one (with a search box) on the right
  let filters = null;
  let panel = null;
  const closePanel = () => {
    if (panel) { panel.remove(); panel = null; document.removeEventListener('pointerdown', outside, true); document.removeEventListener('keydown', onKey, true); }
    filterBtn.classList.remove('open');
  };
  const outside = e => { if (panel && !panel.contains(e.target) && !filterBtn.contains(e.target) && !document.querySelector('.modal')) closePanel(); };
  const onKey = e => { if (e.key === 'Escape' && panel && !document.querySelector('.modal')) { e.stopImmediatePropagation(); closePanel(); } };
  // On a phone the panel covers the page (and the button), so it has its own close button there.
  const touch = () => !!window.matchMedia && matchMedia('(hover: none)').matches;
  const openPanel = async () => {
    if (panel) return closePanel();
    // The panel is there at once; the lists come in when they are loaded.
    const right = h('div', { class: 'fp-right' }, h('div', { class: 'dim', style: 'padding:8px' }, t('Loading…'))), left = h('div', { class: 'fp-left' });
    const me = panel = h('div', { class: 'filterpanel', role: 'dialog', 'aria-label': t('Filter') },
      h('div', { class: 'fp-head' }, h('b', {}, t('Filter')), h('button', { class: 'fp-close', 'aria-label': t('Close'), onclick: closePanel }, '×')), left, right);
    const r = filterBtn.getBoundingClientRect();
    panel.style.left = Math.max(8, Math.min(r.left, innerWidth - 620)) + 'px'; panel.style.top = (r.bottom + 6) + 'px';
    document.body.append(panel);
    filterBtn.classList.add('open');
    document.addEventListener('pointerdown', outside, true);
    document.addEventListener('keydown', onKey, true);
    try { filters = filters || await loadFilters(); }
    catch (e) { closePanel(); return toast(e.message, 6000); }
    if (panel !== me) return; // closed while loading
    const byName = Object.fromEntries(filters.map(f => [f.filter, f]));
    const states = FILTER_GROUP_STATE.map(k => byName[k]).filter(Boolean);
    const main = FILTER_GROUP_MAIN.map(k => byName[k]).filter(Boolean);
    const more = filters.filter(f => !states.includes(f) && !main.includes(f));
    let cur = ''; try { cur = localStorage.getItem(lastKey) || ''; } catch (e) { /* none */ }
    if (!byName[cur] || byName[cur].type === 'boolean') cur = (main[0] || more[0] || {}).filter || '';
    const drawRight = () => {
      const f = byName[cur];
      if (!f) return right.replaceChildren(h('div', { class: 'dim' }, t('None found')));
      const on = st['f.' + f.filter] || '';
      const search = h('input', { type: 'search', placeholder: t('Search {name}…', { name: filterName(f.filter) }), style: 'width:100%' });
      const list = h('div', { class: 'fp-values' });
      const fill = () => {
        const needle = search.value.trim().toLowerCase();
        const all = f.values.filter(x => !needle || valueName(f.filter, x.value).toLowerCase().includes(needle));
        // Thousands of names (actors, directors) are not all drawn: the search narrows them down.
        const vals = all.slice(0, 300);
        list.replaceChildren(...(vals.length ? vals.map(x => h('button', { class: 'fp-val' + (x.value === on ? ' on' : ''), onclick: () => setFilter(f.filter, x.value === on ? '' : x.value) },
          h('span', {}, valueName(f.filter, x.value)), h('small', { class: 'dim' }, String(x.count)))) : [h('div', { class: 'dim', style: 'padding:8px' }, t('None found'))]),
          ...(all.length > vals.length ? [h('div', { class: 'dim', style: 'padding:8px' }, t('{n} more — type to narrow down', { n: all.length - vals.length }))] : []));
      };
      search.oninput = fill; fill();
      right.replaceChildren(search, list);
      // Not on a phone: the keyboard would cover half the list and move the page.
      if (!touch()) setTimeout(() => search.focus(), 0);
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

// Takes down the library page drawn last (its scroll handlers and grid), also when the same page is drawn again.
let LIBCLEAN = null;
window.addEventListener('hashchange', () => { if (LIBCLEAN) LIBCLEAN(); });
async function sectionPage(id, qs) {
  markNav('section/' + id);
  const myHash = location.hash;
  const sec = S.sections.find(s => String(s.id) === String(id));
  if (!sec) return set(h('div', { class: 'empty' }, t('Library not found.')));
  let p = new URLSearchParams(qs || '');
  // Opened without a choice: the filters and sorting of the last visit come back.
  if (!qs) { try { const last = localStorage.getItem('libstate:' + id); if (last) p = new URLSearchParams(last); } catch (e) { /* none */ } }
  // The library's own pages: Recommended, Library, Collections, Playlists, Categories.
  const tab = p.get('tab') || '';
  const tabs = h('div', { class: 'tabs' }, [['recommended', t('Recommended')], ['', t('Library')], ['collections', t('Collections')], ['playlists', t('Playlists')], ['categories', t('Categories')]]
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
  const listUrl = (start, size = PAGE_SIZE) => { const q = filterQuery(); q.set('sort', sortParam); q.set('start', String(start)); q.set('size', String(size)); return `/library/sections/${id}/all?${q}`; };
  const first = (await GET(listUrl(0))).MediaContainer;
  const total = first.totalSize;
  const pick = (key, opts, title) => {
    const cur = opts.find(o => o[0] === st[key]) || opts[0];
    return dropBtn(cur[1], () => opts.map(([v, l]) => ({ label: l, checked: st[key] === v, run: () => { st[key] = v; if (key === 'type') { st.sort = ''; st.rules = ''; for (const k of Object.keys(st)) if (k.startsWith('f.')) delete st[k]; } go(); } })), { title, on: key !== 'sort' && !!st[key] });
  };
  const {filterBtn, chips, closePanel} = filterUI({ st, go, advRules, lastKey: 'libFilterField:' + sec.type + ':' + (st.type || ''), prefetch: true,
    loadFilters: () => cachedFilters(`/library/sections/${id}/filters?includeAdvanced=1${st.type ? '&type=' + st.type : ''}`) });
  const text = (key, ph, w) => h('input', { placeholder: ph, value: st[key], style: `width:${w}px`, onchange: e => { st[key] = e.target.value.trim(); go(); } });
  // ----- sorting: the field and its direction
  const sortFields = (st.type === 'season' || st.type === 'episode' ? [['show', 'TV show']] : []).concat(SORT_FIELDS);
  const sortBtn = dropBtn(sortName(sortNow.field), () => sortFields.map(([f]) => ({ label: sortName(f), checked: f === sortNow.field, run: () => { st.sort = f + ':' + (SORT_ASC.has(f) ? 'asc' : 'desc'); go(); } })), { title: t('Sort') });
  const dirBtn = h('button', { class: 'dirbtn', title: sortNow.desc ? t('Descending') : t('Ascending'), 'aria-label': sortNow.desc ? t('Descending') : t('Ascending'),
    onclick: () => { st.sort = sortNow.field + ':' + (sortNow.desc ? 'asc' : 'desc'); go(); } }, sortNow.desc ? '↓' : '↑');
  // ----- the dice: one title at random from what the filters leave
  const randomPick = async () => {
    try {
      const it = meta(await GET(listUrl(Math.floor(Math.random() * total), 1)))[0];
      if (it) randomDialog(it, randomPick);
    } catch (e) { toast(e.message); }
  };
  const diceBtn = total > 1 ? h('button', { class: 'dirbtn', title: t('Pick one at random'), 'aria-label': t('Pick one at random'), onclick: randomPick }, '🎲') : null;
  // ----- something short from this library: what is not watched yet and fits into the time there is
  const shortBtn = sec.type === 'movie' || sec.type === 'show' ? h('button', { class: 'dirbtn', title: t('Something short'), 'aria-label': t('Something short'), onclick: () => shortDialog(sec) }, '⏱') : null;
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
        grid._onNew = els => els.forEach(el => more.observe(el));
        grid._onGone = els => els.forEach(el => { more.unobserve(el); visible.delete(el); });
        grid._reserve(total - r.Metadata.length);
      } else grid._put(p * PAGE_SIZE, r.Metadata);
      have.add(p);
    } catch (e) {
      if (e.name === 'AbortError') return; // another page was opened
      // Tried again when the page is scrolled to the next time.
      toast(e.message);
      if (grid) for (let i = p * PAGE_SIZE; i < Math.min(total, (p + 1) * PAGE_SIZE); i++) { const el = grid._card(i); if (el && el.classList.contains('ph')) more.observe(el); }
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
  let forced = null, forcedAt = 0; // the letter just clicked stays lit for a moment, whatever is under the bar
  const jump = async offset => {
    const i = Math.min(offset, total - 1);
    const at = () => {
      if (!grid) return null;
      const head = sticky.getBoundingClientRect().bottom;
      window.scrollTo({ top: grid._top(i) - head - 12, behavior: 'auto' });
      grid._sync(); // the cards there are made at once, not on the next frame
      return window.scrollY;
    };
    forced = offset; forcedAt = Date.now();
    const y = at();
    await loadPage(pageOf(i));
    // The page's real cards may be a little taller than the empty ones: line up again, if nobody scrolled meanwhile.
    if (y !== null && Math.abs(window.scrollY - y) < 3) at();
  };
  // The letters at the side (only when sorted by title).
  const letters = h('div', { class: 'letters' });
  const marks = []; // {btn, ranges}: the parts of the list a letter stands for
  if (sortNow.field === 'title' && total > PAGE_SIZE / 2) {
    GET(`/library/sections/${id}/firstCharacter?${filterQuery()}`).then(r => {
      const dirs = r.MediaContainer.Directory; let sum = 0;
      // A letter can have several runs in the list (Ä/Ö/Ü sort behind Z): one button, all its places.
      const byKey = new Map();
      for (const d of dirs) {
        // The server says where a run begins and ends in the A-Z list (an older one only counts the titles).
        const from = d.start !== undefined ? d.start : sum, to = d.end !== undefined ? d.end : sum + d.size;
        sum += d.size;
        const run = [Math.max(0, sortNow.desc ? total - to : from), sortNow.desc ? total - from : to];
        const m = byKey.get(d.key) || { key: d.key, size: 0, ranges: [] };
        m.size += d.size; m.ranges.push(run); byKey.set(d.key, m);
      }
      letters.replaceChildren(...[...byKey.values()].map(m => {
        const lo = Math.min(...m.ranges.map(r => r[0]));
        const btn = h('button', { title: `${m.key} · ${m.size}`, onclick: () => jump(lo) }, m.key);
        marks.push({ btn, ranges: m.ranges });
        return btn;
      }));
      showPos();
    }).catch(() => {});
  }
  // On a phone the letters are swiped: the finger runs down the bar and the list follows the letter under it.
  // The big letter shows in the middle of the screen, a little above the finger: at the edge it was under the thumb.
  const bubble = h('div', { class: 'letterbubble', 'aria-hidden': 'true' });
  let scrubbing = false, scrubKey = null;
  const scrubAt = y => {
    bubble.style.top = Math.round(Math.max(70, Math.min(innerHeight - 170, y - 110))) + 'px';
    let best = null, dist = Infinity;
    for (const m of marks) {
      const r = m.btn.getBoundingClientRect(), d = Math.abs(y - (r.top + r.height / 2));
      if (d < dist) { dist = d; best = m; }
    }
    if (!best || best === scrubKey) return;
    scrubKey = best;
    bubble.textContent = best.btn.textContent;
    jump(Math.min(...best.ranges.map(r => r[0])));
  };
  // The bar sits where a thumb scrolls the page, so it only takes the finger after it rests on it a moment
  // (HOLD ms); a finger that moves before that scrolls the page as usual. A tap still jumps to the letter.
  const HOLD = 180;
  let holdTimer = 0, holdY = 0, lastY = 0;
  const scrubEnd = () => { clearTimeout(holdTimer); holdTimer = 0; scrubbing = false; letters.classList.remove('scrub'); bubble.remove(); bubble.textContent = ''; };
  letters.addEventListener('touchstart', e => {
    scrubEnd();
    if (e.touches.length !== 1 || !marks.length) return;
    holdY = lastY = e.touches[0].clientY;
    holdTimer = setTimeout(() => {
      holdTimer = 0; scrubbing = true; scrubKey = null;
      letters.classList.add('scrub');
      document.body.append(bubble);
      scrubAt(lastY);
    }, HOLD);
  }, { passive: true });
  letters.addEventListener('touchmove', e => {
    lastY = e.touches[0].clientY;
    if (scrubbing) {
      if (!e.cancelable) { scrubEnd(); return; } // the page already scrolls
      e.preventDefault();
      scrubAt(lastY);
    } else if (holdTimer && Math.abs(lastY - holdY) > 8) scrubEnd(); // moved at once: scrolling the page
  }, { passive: false });
  letters.addEventListener('touchend', e => { if (scrubbing) e.preventDefault(); scrubEnd(); }); // no click after a swipe
  letters.addEventListener('touchcancel', scrubEnd);
  letters.addEventListener('contextmenu', e => { if (scrubbing) e.preventDefault(); });
  const countEl = h('span', { class: 'count', title: t('{n} shown', { n: total }) }, itemCount(total));
  // The controls used less often fold away on a phone behind "More", so the row fits the screen.
  const opt = el => { if (el) el.classList.add('opt'); return el; };
  const optSet = !!(st.from || st.to || st.container || st.q);
  const tools = h('div', { class: 'bar-tools' + (optSet ? ' open' : '') },
    sec.type === 'show' ? pick('type', [['', t('Shows')], ['season', t('Seasons')], ['episode', t('Episodes')]]) : null,
    sec.type === 'music' ? pick('type', [['', t('Artists')], ['album', t('Albums')], ['track', t('Songs')]]) : null,
    filterBtn, sortBtn, dirBtn, diceBtn, shortBtn,
    h('button', { class: 'dd toolsmore' + (optSet ? ' on' : ''), 'aria-expanded': String(optSet), onclick: e => {
      const open = tools.classList.toggle('open'); e.currentTarget.setAttribute('aria-expanded', String(open));
    } }, t('More'), icon('down')),
    opt(text('from', t('Year from'), 95)), opt(text('to', t('Year to'), 85)),
    opt(pick('container', [['', t('Any file type')], ...['mkv', 'mp4', 'avi', 'mov', 'ts', 'm2ts', 'wmv', 'webm', 'mpg'].map(c => [c, c.toUpperCase()])])),
    opt(clearable(text('q', t('Filter by title'), 150), () => { st.q = ''; go(); })),
    countEl,
    h('span', { class: 'grow' }),
    sizeSlider(),
    S.me.admin ? opt(h('button', { onclick: async () => { await api('POST', `/library/sections/${id}/refresh`); toast(t('Scanning {name}', { name: sec.title })); pollActivity(); } }, t('Scan now'))) : null);
  const sticky = h('div', { class: 'libhead' },
    h('h1', {}, sec.title, h('button', { class: 'more', title: t('More'), 'aria-label': t('More'), onclick: e => sectionMenu(sec, e.currentTarget) }, '⋯')),
    tabs, tools, chips);
  set(sticky, total ? holder : h('div', { class: 'empty' }, t('Nothing matches these filters.')), letters);
  // The letters start below the bar on top (its height changes with the folded tools and the chips).
  const placeLetters = () => letters.style.setProperty('--top', Math.round(sticky.getBoundingClientRect().bottom + 8) + 'px');
  const headSize = new ResizeObserver(placeLetters);
  headSize.observe(sticky);
  placeLetters();
  if (total) {
    // Back from an item: the scroll position (the page is as long as before at once).
    const want = LIBPOS[myHash];
    await loadPage(0, first);
    if (want) window.scrollTo(0, want.y);
    else stagger();
  }
  // Where you are: the first poster under the bar gives the number (in the counter) and the letter (lit in the side list).
  let raf = 0, lastPos = 0, prevY = window.scrollY, dir = 1;
  const ahead = new Set(); // pages fetched ahead once (a failed one is tried again only when scrolled to)
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
    // Scrolling through titles already there: the next page that way is fetched ahead, so it is ready on arrival.
    // Only from a loaded page, so dragging the scroll bar across empty cards still fetches nothing.
    const y = window.scrollY;
    if (y !== prevY) { dir = y > prevY ? 1 : -1; prevY = y; }
    const p = pageOf(pos), next = p + dir;
    if (have.has(p) && next >= 0 && next * PAGE_SIZE < total && !ahead.has(next)) { ahead.add(next); loadPage(next); }
    if (forced !== null && Date.now() - forcedAt < 1500) pos = forced; else forced = null;
    lastPos = pos;
    countEl.textContent = t('{pos} / {total}', { pos: fmtNum(pos + 1), total: fmtNum(total) });
    for (const m of marks) m.btn.classList.toggle('on', m.ranges.some(([lo, hi]) => pos >= lo && pos < hi));
  };
  const onPos = () => { if (!raf) raf = requestAnimationFrame(showPos); };
  window.addEventListener('scroll', onPos, { passive: true });
  let t0 = 0;
  const onScroll = () => {
    if (location.hash !== myHash) return;
    const now = Date.now(); if (now - t0 > 200) { t0 = now; LIBPOS[myHash] = { y: window.scrollY, n: total }; }
  };
  window.addEventListener('scroll', onScroll, { passive: true });
  if (LIBCLEAN) LIBCLEAN();
  LIBCLEAN = () => { LIBCLEAN = null; scrubEnd(); more.disconnect(); headSize.disconnect(); closePanel(); window.removeEventListener('scroll', onScroll); window.removeEventListener('scroll', onPos); saveLibPos(); };
}

const itemCount = n => n === 1 ? t('1 item') : t('{n} items', { n });

// Recommended, Collections, Playlists and Categories of one library.
async function sectionSubPage(sec, tab, tabs) {
  const head = h('h1', {}, sec.title);
  if (tab === 'recommended') {
    const wait = skeletonSoon(() => set(head, tabs, skeletonRows(3)));
    const hubs = (await GET(`/hubs/sections/${sec.id}`)).MediaContainer.Hub || [];
    wait();
    return set(head, tabs, hubs.length ? hubs.map(hb => row(hb.title, hb.Metadata))
      : h('div', { class: 'empty' }, t('Nothing to recommend yet. Rows appear once the library has titles.')));
  }
  const tile = (d, onclick) => h('div', { class: 'card', onclick },
    h('div', { class: 'poster' }, d.thumb ? h('img', { src: withTok(d.thumb), loading: 'lazy', alt: '' }) : null, h('span', { class: 'badge' }, d.childCount)),
    h('div', { class: 't', title: d.title }, d.title), h('div', { class: 's' }, itemCount(d.childCount)));
  const open = (k, v) => () => (location.hash = `#/section/${sec.id}?${k}=${encodeURIComponent(v)}`);
  if (tab === 'collections') {
    const list = (await GET(`/library/sections/${sec.id}/collections`)).MediaContainer.Metadata;
    if (!list.length) return set(head, tabs, h('div', { class: 'empty' }, t('No collections yet. Film series are added when details are downloaded; add your own under Edit details → Collections.')));
    // "Hide watched" (kept per account): each collection counts and opens with only what this person has not seen;
    // the "Unwatched" chip on the opened collection takes it off there again. Fully seen collections go to the end.
    const grid = h('div', { class: 'grid' });
    const draw = () => {
      const hide = store.get('colHideWatched', '0') === '1';
      const left = d => d.childCount - (d.viewedLeafCount || 0);
      const shown = hide ? [...list.filter(d => left(d) > 0), ...list.filter(d => left(d) <= 0)] : list;
      grid.replaceChildren(...shown.map(d => {
        const el = tile(hide ? { ...d, childCount: left(d) } : d, () => (location.hash = `#/section/${sec.id}?f.collection=${encodeURIComponent(d.title)}${hide ? '&f.unwatched=true' : ''}`));
        if (hide) {
          el.querySelector('.s').textContent = left(d) > 0 ? t('{n} of {total} not seen', { n: left(d), total: d.childCount }) : t('All seen');
          if (left(d) <= 0) el.style.opacity = '0.5';
        }
        return el;
      }));
    };
    const box = h('input', { type: 'checkbox', checked: store.get('colHideWatched', '0') === '1', onchange: e => { store.set('colHideWatched', e.target.checked ? '1' : '0'); draw(); } });
    draw();
    return set(head, tabs, h('label', { style: 'display:inline-flex;gap:8px;align-items:center;margin:0 0 12px' }, box, t('Hide watched')), grid);
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
// A box with posters to pick from closes when one is opened or played (not for its "⋯" menu or the pencil).
function closeOnOpen(box) {
  box.addEventListener('click', e => {
    const el = /** @type {Element} */ (e.target);
    if (el.closest('a[href^="#/"]') && !el.closest('button.more, button.edit, .pick')) box.remove();
  }, true);
}

// The dice of a library page: the title picked, with its description, to play, to open, or to throw again.
function randomDialog(it, again) {
  const at = resumeAt(it);
  const name = it.type === 'episode' ? `${it.grandparentTitle} · ${it.title}` : it.type === 'season' ? `${it.parentTitle} · ${seasonTitle(it.title)}` : it.title;
  const facts = [it.year, it.type === 'show' ? t('{n} episodes', { n: it.leafCount || 0 }) : it.duration ? fmtDur(it.duration) : '', ...(it.genres || []).slice(0, 3)].filter(Boolean).join(' · ');
  const close = () => box.remove();
  const playBtn = isPlayable(it) && !it.missing ? h('button', { class: 'primary', onclick: () => { close(); play(it, at); } }, at ? t('▶ Resume from {time}', { time: fmtTime(at) }) : t('▶ Play'))
    : (it.type === 'show' || it.type === 'season') && it.leafCount ? h('button', { class: 'primary', onclick: () => { close(); playNextEpisode(it.id); } }, t('▶ Play next episode')) : null;
  const box = modal(t('At random'), [
    h('div', { class: 'pickone' }, posterCard(it),
      h('div', {}, h('h3', {}, name), facts ? h('div', { class: 'dim' }, facts) : null, it.summary ? h('p', { class: 'pick-sum' }, it.summary) : null)),
    h('div', { class: 'actions', style: 'margin-bottom:0' }, playBtn,
      h('button', { onclick: () => { close(); location.hash = '#/item/' + it.id; } }, t('More info')),
      h('button', { onclick: () => { close(); again(); } }, t('🎲 Another one')),
      h('button', { onclick: close }, t('Close')))]);
  closeOnOpen(box);
}

// "Something short": say how much time there is, and get what fits into it from what you are watching
// (titles begun count with the time left) and from your watchlist; from a library's page (sec), what in that
// library is not watched yet and fits, in random order (a show library: its episodes).
const SHORT_TIMES = [20, 30, 45, 60, 90];
function shortDialog(sec) {
  let minutes = Number(store.get('shortMinutes', '30')) || 30;
  const list = h('div', { class: 'picklist' });
  const times = h('div', { class: 'picktimes' });
  const own = h('input', { type: 'number', min: '1', max: '600', style: 'width:80px', 'aria-label': t('Minutes'),
    onchange: e => { const v = Math.round(Number(e.target.value)); if (v > 0) choose(Math.min(600, v)); } });
  let asked = 0;
  const choose = async m => {
    minutes = m; store.set('shortMinutes', String(m));
    times.replaceChildren(...SHORT_TIMES.map(v => h('button', { class: 'small' + (v === m ? ' primary' : ''), 'aria-pressed': String(v === m), onclick: () => choose(v) },t('{m} min', { m: v }))),
      h('label', { style: 'display:inline-flex;gap:6px;align-items:center' }, own, t('min')));
    own.value = SHORT_TIMES.includes(m) ? '' : String(m);
    const mine = ++asked;
    list.replaceChildren(h('div', { class: 'dim' }, t('Loading…')));
    try {
      let url = `/api/short?minutes=${m}`;
      if (sec) {
        const q = new URLSearchParams({ sort: 'random:asc', start: '0', size: '30', 'duration>': '1', 'duration<': String(m), unwatched: 'true' });
        if (sec.type === 'show') q.set('type', 'episode');
        url = `/library/sections/${sec.id}/all?${q}`;
      }
      const items = meta(await GET(url));
      if (mine !== asked) return;
      const left = it => Math.max(0, (it.duration || 0) - (it.viewOffset || 0));
      list.replaceChildren(...(items.length ? items.map(it => posterCard(it, { caption: x => x.viewOffset ? t('{time} left', { time: fmtDur(left(x)) }) : fmtDur(x.duration) }))
        : [h('div', { class: 'dim' }, sec ? t('Nothing in “{name}” that you have not watched fits into {m} minutes.', { name: sec.title, m }) : t('Nothing you are watching or have on your watchlist fits into {m} minutes.', { m }))]));
    } catch (e) { if (mine === asked) list.replaceChildren(h('div', { class: 'dim' }, e.message)); }
  };
  const box = modal(t('Something short'), [
    h('p', { class: 'dim', style: 'margin-top:-6px' }, sec ? t('How much time do you have? What fits comes from “{name}”, not watched yet, in random order.', { name: sec.title }) : t('How much time do you have? What fits comes from what you are watching and from your watchlist.')),
    times, list,
    h('div', { class: 'actions', style: 'margin-bottom:0' }, h('button', { onclick: () => box.remove() }, t('Close')))]);
  closeOnOpen(box);
  choose(minutes);
}

// The server's own playlists have English names; show them in the chosen language.
const plTitle = pl => pl.default ? t(pl.title) : pl.title;
