// Lichtspiel web app, part 2: playlists, smart playlist editor, admin pages, API page, router.
'use strict';

// ---------- playlists
const kindName = k => ({ manual: t('normal'), smart: t('smart'), default: t('default') }[k] || k);

async function playlistsPage(qs) {
  markNav('playlists');
  const p = new URLSearchParams(qs || '');
  const st = { kind: p.get('kind') || '', owner: p.get('owner') || '', sort: p.get('sort') || 'pinned:desc,title', title: p.get('title') || '' };
  const go = () => { const n = new URLSearchParams(); for (const [k, v] of Object.entries(st)) if (v) n.set(k, v); location.hash = '#/playlists?' + n; };
  const q = new URLSearchParams(); for (const [k, v] of Object.entries(st)) if (v) q.set(k, v);
  const list = meta(await GET('/playlists?' + q));
  const sel = (key, opts) => h('select', { onchange: e => { st[key] = e.target.value; go(); } }, opts.map(([v, l]) => h('option', { value: v, selected: st[key] === v }, l)));
  set(h('h1', {}, t('Playlists')),
    h('div', { class: 'bar-tools' },
      sel('kind', [['', t('All kinds')], ['default', t('Default')], ['smart', t('Smart')], ['manual', t('Normal')]]),
      sel('owner', [['', t('Everyone\'s')], ['me', t('Mine')], ['others', t('Shared with me')], ['server', t('From the server')]]),
      sel('sort', [['pinned:desc,title', t('Pinned first')], ['title', t('Name A–Z')], ['title:desc', t('Name Z–A')], ['updated_at:desc', t('Recently changed')], ['created_at:desc', t('Newest')], ['item_count:desc', t('Most items')], ['duration:desc', t('Longest')]]),
      h('input', { placeholder: t('Find by name'), value: st.title, onchange: e => { st.title = e.target.value.trim(); go(); } }),
      h('span', { class: 'grow' }),
      h('button', { onclick: newManual }, t('+ New playlist')),
      h('button', { class: 'primary', onclick: () => smartEditor(null) }, t('+ New smart playlist'))),
    list.length ? h('div', { class: 'list' }, list.map(pl => h('div', { class: 'li' },
      h('img', pl.thumb ? { src: withTok(pl.thumb) } : {}),
      h('div', { class: 'grow' }, h('a', { href: '#/playlist/' + pl.id }, h('b', {}, plTitle(pl))), ' ',
        h('span', { class: 'tag ' + pl.kind }, kindName(pl.kind)), pl.shared && pl.kind !== 'default' ? h('span', { class: 'tag' }, t('shared')) : null, pl.pinned ? h('span', { class: 'tag' }, t('on home')) : null,
        h('div', { class: 'dim' }, t(pl.owner === 'server' ? '{n} items · {length} · from the server · changed {date}' : '{n} items · {length} · by {owner} · changed {date}', { n: pl.leafCount, length: fmtDur(pl.duration) || t('{m} min', { m: 0 }), owner: pl.owner, date: fmtDate(pl.updatedAt) }))),
      pl.leafCount ? h('button', { class: 'primary small', onclick: () => playPlaylist(pl.id) }, t('▶ Play')) : null))) : h('div', { class: 'empty' }, t('No playlists match.')));
}

async function newManual() {
  const name = await ask(t('Name of the new playlist'));
  if (!name) return;
  const r = await api('POST', '/playlists', { title: name, kind: 'manual' });
  location.hash = '#/playlist/' + meta(r)[0].id;
}

async function playPlaylist(id, shuffle) {
  const items = meta(await GET(`/playlists/${id}/items${shuffle ? '?sort=random' : ''}`));
  if (!items.length) return toast(t('This playlist is empty'));
  const [first, ...rest] = items;
  play(first, first.viewOffset ? first.viewOffset / 1000 : 0, rest);
}

async function playlistPage(id, qs) {
  markNav('playlists');
  const pl = meta(await GET('/playlists/' + id))[0];
  const p = new URLSearchParams(qs || '');
  const st = { sort: p.get('sort') || '', type: p.get('type') || '', watched: p.get('watched') || '', q: p.get('q') || '' };
  const go = () => { const n = new URLSearchParams(); for (const [k, v] of Object.entries(st)) if (v) n.set(k, v); location.hash = `#/playlist/${id}?${n}`; };
  const q = new URLSearchParams();
  if (st.sort) q.set('sort', st.sort);
  if (st.type) q.set('kind', st.type);
  if (st.watched) q.set('watched', st.watched === 'watched' ? 'true' : 'false');
  if (st.q) q.set('q', st.q);
  const res = (await GET(`/playlists/${id}/items?${q}`)).MediaContainer;
  const manual = pl.kind === 'manual';
  const reorder = manual && !st.sort && !st.type && !st.watched && !st.q && pl.editable;
  const sel = (key, opts) => h('select', { onchange: e => { st[key] = e.target.value; go(); } }, opts.map(([v, l]) => h('option', { value: v, selected: st[key] === v }, l)));
  const items = res.Metadata;
  const reload = () => playlistPage(id, qs);
  const move = async (idx, dir) => {
    const target = idx + dir; if (target < 0 || target >= items.length) return;
    const after = dir < 0 ? (target === 0 ? 0 : items[target - 1].id) : items[target].id;
    await api('PUT', `/playlists/${id}/items/${items[idx].id}/move?after=${after}`); reload();
  };
  set(h('div', { class: 'dim' }, h('a', { href: '#/playlists' }, t('Playlists')), ' ›'),
    h('h1', {}, plTitle(pl), ' ', h('span', { class: 'tag ' + pl.kind }, kindName(pl.kind))),
    pl.summary ? h('p', { class: 'dim' }, pl.default ? t(pl.summary) : pl.summary) : null,
    h('div', { class: 'actions' },
      h('button', { class: 'primary', onclick: () => playPlaylist(id) }, t('▶ Play all')),
      h('button', { onclick: () => playPlaylist(id, true) }, t('Shuffle')),
      pl.smart && pl.editable ? h('button', { onclick: () => smartEditor(pl) }, t('Edit rules')) : null,
      pl.editable ? h('button', { onclick: async () => { const name = await ask(t('New name'), { value: plTitle(pl) }); if (name) { await api('PUT', '/playlists/' + id, { title: name }); reload(); } } }, t('Rename')) : null,
      pl.editable ? h('button', { onclick: async () => { await api('PUT', '/playlists/' + id, { pinned: !pl.pinned }); reload(); } }, pl.pinned ? t('Remove from home') : t('Show on home')) : null,
      pl.editable && !pl.default ? h('button', { onclick: async () => { await api('PUT', '/playlists/' + id, { shared: !pl.shared }); reload(); } }, pl.shared ? t('Stop sharing') : t('Share with everyone')) : null,
      pl.smart ? h('button', { onclick: async () => { const r = await api('POST', `/playlists/${id}/copy`, {}); location.hash = '#/playlist/' + r.id; } }, t('Save as normal playlist')) : null,
      pl.editable && !pl.default ? h('button', { class: 'danger', onclick: async () => { if (await sure(t('Delete this playlist?'), { ok: t('Delete'), danger: true })) { await api('DELETE', '/playlists/' + id); location.hash = '#/playlists'; } } }, t('Delete')) : null),
    h('div', { class: 'bar-tools' },
      sel('sort', [['', manual ? t('Playlist order') : t('Rule order')], ...SORTS, ['episode_order', t('Show, season, episode')]]),
      sel('type', [['', t('Movies and episodes')], ['movie', t('Movies')], ['episode', t('Episodes')]]),
      sel('watched', [['', t('Watched or not')], ['unwatched', t('Unwatched')], ['watched', t('Watched')]]),
      h('input', { placeholder: t('Filter by title'), value: st.q, onchange: e => { st.q = e.target.value.trim(); go(); } }),
      h('span', { class: 'grow' }), h('span', { class: 'dim' }, t('{shown} of {all} items · {length}', { shown: res.totalSize, all: pl.leafCount, length: fmtDur(pl.duration) }))),
    items.length ? h('div', { class: 'list' }, items.map((it, i) => h('div', { class: 'li' },
      h('img', { src: withTok(it.type === 'episode' && it.grandparentThumb ? it.grandparentThumb : it.thumb), loading: 'lazy' }),
      h('div', { class: 'grow' }, h('a', { href: '#/item/' + it.id }, h('b', {}, it.type === 'episode' ? `${it.grandparentTitle} · ${t('S{n}', { n: it.parentIndex })}${t('E{n}', { n: it.index })} ${it.title}` : it.title)),
        h('div', { class: 'dim' }, [it.year, fmtDur(it.duration), it.viewCount ? t('✓ watched') : '', it.viewOffset ? t('stopped at {time}', { time: fmtTime(it.viewOffset / 1000) }) : ''].filter(Boolean).join(' · '))),
      reorder ? [h('button', { class: 'small', title: t('Move up'), onclick: () => move(i, -1) }, '↑'), h('button', { class: 'small', title: t('Move down'), onclick: () => move(i, 1) }, '↓')] : null,
      manual && pl.editable ? h('button', { class: 'small danger', onclick: async () => { await api('DELETE', `/playlists/${id}/items/${it.id}`); reload(); } }, t('Remove')) : null,
      h('button', { class: 'primary small', onclick: () => play(it, it.viewOffset ? it.viewOffset / 1000 : 0, items.slice(i + 1)) }, '▶')))) :
      h('div', { class: 'empty' }, manual ? t('Empty. Open a movie or episode and use "Add to playlist".') : t('Nothing matches these rules right now.')));
}

function smartEditor(pl) {
  const fields = ((S.filters && S.filters.fields) || []).filter(f => f.type !== 'sort');
  const ops = (S.filters && S.filters.operators) || [];
  const q = pl && pl.query ? JSON.parse(JSON.stringify(pl.query)) : { match_all: true, rules: [{ field: 'kind', op: 'is', value: 'movie' }], sort: 'title', limit: null, sections: [] };
  const rulesBox = h('div');
  const preview = h('div', { class: 'dim', style: 'margin-top:10px' });
  const OPS = { bool: ['is', 'is_not'], date: ['in_last_days', 'not_in_last_days', 'gt', 'lt'], number: ['is', 'is_not', 'gt', 'gte', 'lt', 'lte', 'between', 'in'], text: ['is', 'is_not', 'contains', 'not_contains', 'starts_with', 'ends_with', 'in'] };
  const opsFor = type => ops.filter(o => (OPS[type] || OPS.text).includes(o.op));
  let timer;
  const upd = () => { clearTimeout(timer); timer = setTimeout(async () => {
    try {
      const r = (await api('POST', '/playlists/preview', { query: q })).MediaContainer;
      preview.textContent = t('{n} items match: ', { n: r.totalSize }) + r.Metadata.slice(0, 6).map(m => m.type === 'episode' ? `${m.grandparentTitle} ${t('S{n}', { n: m.parentIndex })}${t('E{n}', { n: m.index })}` : m.title).join(', ') + (r.totalSize > 6 ? ' …' : '');
    } catch (e) { preview.textContent = e.message; }
  }, 250); };
  const drawRules = () => {
    rulesBox.replaceChildren(...q.rules.map((r, i) => {
      const f = fields.find(x => x.field === r.field) || fields[0];
      const valEl = f.type === 'bool'
        ? h('select', { onchange: e => { r.value = e.target.value === 'true'; upd(); } }, h('option', { value: 'true', selected: r.value === true }, t('yes')), h('option', { value: 'false', selected: r.value === false }, t('no')))
        : h('input', { value: Array.isArray(r.value) ? r.value.join(',') : (r.value ?? ''), placeholder: r.op === 'between' ? t('low,high') : r.op.includes('days') ? t('days') : t('value'),
          onchange: e => { const v = e.target.value; r.value = r.op === 'between' ? v.split(',').map(Number) : (f.type === 'number' || r.op.includes('days')) && v !== '' && !isNaN(v) ? Number(v) : v; upd(); } });
      return h('div', { class: 'rule' },
        h('select', { onchange: e => { r.field = e.target.value; const nf = fields.find(x => x.field === r.field); r.op = opsFor(nf.type)[0].op; r.value = nf.type === 'bool' ? true : ''; drawRules(); upd(); } }, fields.map(x => h('option', { value: x.field, selected: x.field === r.field }, t(x.label)))),
        h('select', { onchange: e => { r.op = e.target.value; drawRules(); upd(); } }, opsFor(f.type).map(o => h('option', { value: o.op, selected: o.op === r.op }, t(o.label)))),
        valEl,
        h('button', { class: 'small danger', onclick: () => { q.rules.splice(i, 1); drawRules(); upd(); } }, '✕'));
    }));
  };
  const title = h('input', { value: pl ? plTitle(pl) : '', placeholder: t('e.g. Unwatched sci-fi from the 90s'), style: 'width:100%' });
  const shared = h('input', { type: 'checkbox', checked: pl ? pl.shared : false });
  const pinned = h('input', { type: 'checkbox', checked: pl ? pl.pinned : false });
  const box = modal(pl ? t('Edit smart playlist') : t('New smart playlist'), [
    formRow(t('Name'), title),
    formRow(t('Include items that match'),
      h('select', { onchange: e => { q.match_all = e.target.value === 'all'; upd(); } }, h('option', { value: 'all', selected: q.match_all }, t('all of these rules')), h('option', { value: 'any', selected: !q.match_all }, t('any of these rules')))),
    h('div', { style: 'margin-top:10px' }, rulesBox),
    h('button', { class: 'small', onclick: () => { q.rules.push({ field: 'genre', op: 'is', value: '' }); drawRules(); } }, t('+ Add rule')),
    formRow(t('Sort by'),
      h('select', { onchange: e => { q.sort = e.target.value; upd(); } }, [...SORTS, ['episode_order', t('Show, season, episode')]].map(([v, l]) => h('option', { value: v, selected: q.sort === v }, l)))),
    formRow(t('Limit (empty = no limit)'),
      h('input', { type: 'number', min: 1, value: q.limit || '', onchange: e => { q.limit = e.target.value ? Number(e.target.value) : null; upd(); } })),
    formRow(t('Libraries'), h('div', { class: 'checks' }, S.sections.map(s => h('label', {}, h('input', { type: 'checkbox', checked: (q.sections || []).includes(s.id), onchange: e => { q.sections = (q.sections || []).filter(x => x !== s.id); if (e.target.checked) q.sections.push(s.id); upd(); } }), s.title)), h('span', { class: 'dim' }, t('(none ticked = all)')))),
    pl && pl.default ? null : h('div', { class: 'checks', style: 'margin-top:10px' }, h('label', {}, shared, t('Share with everyone')), h('label', {}, pinned, t('Show on home'))),
    preview,
    h('div', { class: 'actions' }, h('button', { class: 'primary', onclick: async () => {
      if (!title.value.trim()) return toast(t('Give it a name'));
      // A default playlist keeps its English name unless it was really renamed.
      const name = pl && pl.default && title.value === plTitle(pl) ? pl.title : title.value;
      try {
        if (pl) { await api('PUT', '/playlists/' + pl.id, { title: name, query: q, shared: shared.checked, pinned: pinned.checked }); box.remove(); playlistPage(pl.id); }
        else { const r = await api('POST', '/playlists', { title: name, kind: 'smart', query: q, shared: shared.checked, pinned: pinned.checked }); box.remove(); location.hash = '#/playlist/' + meta(r)[0].id; }
      } catch (e) { toast(e.message, 5000); }
    } }, t('Save')), h('button', { onclick: () => box.remove() }, t('Cancel')))]);
  drawRules(); upd();
}

// ---------- admin: libraries
const sectionKind = k => ({ movie: t('Movies'), show: t('TV shows'), other: t('Other videos') }[k] || k);

async function librariesPage() {
  const secs = (await GET('/library/sections')).MediaContainer.Directory;
  S.sections = secs; drawChrome(); markNav('libraries');
  const name = h('input', { placeholder: t('Movies'), style: 'width:100%;max-width:420px' });
  const type = h('select', {}, h('option', { value: 'movie' }, t('Movies')), h('option', { value: 'show' }, t('TV shows')), h('option', { value: 'other' }, t('Other videos')));
  const paths = h('textarea', { placeholder: t('One folder per line, e.g. /media/Movies or /volume1/video/Movies'), style: 'min-height:70px;max-width:620px' });
  const st = await GET('/api/settings');
  const langs = st.languages;
  const serverLang = (langs.find(l => l.code === st.settings.metadata_language) || {}).name || st.settings.metadata_language;
  const langSel = (cur, list, onchange) => h('select', { onchange }, h('option', { value: '', selected: !cur }, t('Same as server ({lang})', { lang: serverLang })),
    list.map(l => h('option', { value: l.code, selected: cur === l.code }, l.name)));
  const newLang = langSel('', langs);
  // The list redraws on its own (the form below is left alone) and keeps checking
  // while a library is being scanned, so new and rescanned libraries show up without a reload.
  const listBox = h('div', {});
  let polling = false;
  // more: check again this many times even if nothing is scanning yet (the scan
  // starts on its own thread right after the request).
  const refreshList = async (more = 0) => {
    if (!listBox.isConnected) return;
    let fresh;
    try { fresh = (await GET('/library/sections')).MediaContainer.Directory; } catch (e) { return; }
    if (!listBox.isConnected) return;
    S.sections = fresh; drawChrome(); markNav('libraries');
    drawList(fresh);
    if ((more > 0 || fresh.some(s => s.scanning)) && !polling) {
      polling = true;
      setTimeout(() => { polling = false; refreshList(Math.max(0, more - 1)); }, 2000);
    }
  };
  const drawList = secs => listBox.replaceChildren(...(secs.length ? [h('div', { class: 'list' }, secs.map(s => h('div', { class: 'li' },
      h('div', { class: 'grow' }, h('b', {}, s.title), ' ', h('span', { class: 'tag' }, sectionKind(s.type)), s.scanning ? h('span', { class: 'tag smart' }, t('scanning…')) : null,
        h('div', { class: 'dim' }, (s.type === 'show' ? t('{n} shows', { n: s.count }) + ', ' : '') + t('{n} videos · last scan {when}', { n: s.videoCount, when: s.scannedAt ? fmtDateTime(s.scannedAt) : t('never') })),
        h('div', { class: 'dim', style: 'font-size:13px' }, s.paths.join(' · ')),
        h('div', { style: 'display:flex;gap:18px;flex-wrap:wrap;align-items:center;margin-top:6px' },
          h('label', { style: 'display:flex;gap:6px;align-items:center;margin:0;color:var(--text)', title: t('Detect credits and allow credits skipping for items in this library when enabled in server settings.') },
            h('input', { type: 'checkbox', checked: s.creditsDetection !== false, onchange: async e => { try { await api('PUT', '/library/sections/' + s.id, { credits_detection: e.target.checked }); toast(t('Saved')); } catch (er) { toast(er.message); } } }), t('Enable credits detection')),
          h('label', { style: 'display:flex;gap:6px;align-items:center;margin:0;color:var(--text)', title: t('Language of titles, plots and episode names downloaded for this library') }, t('Details language'),
            langSel(s.language || '', langs, async e => {
              try { const r = await api('PUT', '/library/sections/' + s.id, { language: e.target.value }); toast(r.refetching ? t('Downloading details for {n} movies and shows again — progress is shown at the top', { n: r.refetching }) : t('Saved'), 6000); }
              catch (er) { toast(er.message); }
            })))),
      h('button', { onclick: async () => { await api('POST', `/library/sections/${s.id}/refresh`); toast(t('Scanning {name}', { name: s.title })); refreshList(2); pollActivity(); } }, t('Scan now')),
      h('button', { title: t('Look for end credits in every video of this library now (from the chapters in the files)'), onclick: async () => {
        try { const r = await api('POST', `/library/sections/${s.id}/credits`); toast(t('Checking {n} videos for credits — progress is shown at the top', { n: r.videos }), 5000); pollActivity(); } catch (er) { toast(er.message, 6000); }
      } }, t('Detect credits')),
      h('button', { onclick: () => foldersDialog(s) }, t('Folders')),
      h('button', { title: t('More'), 'aria-label': t('More'), onclick: e => sectionMenu(s, e.currentTarget) }, '⋯'),
      h('button', { class: 'danger', onclick: async () => { if (await sure(t('Remove the library "{name}"? Your files stay on disk.', { name: s.title }), { ok: t('Remove'), danger: true })) { await api('DELETE', '/library/sections/' + s.id); refreshList(); } } }, t('Remove')))))] : []));
  drawList(secs);
  set(h('h1', {}, t('Libraries')),
    h('p', { class: 'dim' }, t('Movies and TV shows are kept in separate libraries. Name TV files like "Show/Season 01/Show - S01E02 - Title.mkv" and movies like "Title (Year).mkv". Every video container is picked up: '), h('code', {}, 'mkv mp4 avi mov ts m2ts wmv webm mpg vob flv ogv 3gp …')),
    listBox,
    h('div', { class: 'panel', style: 'margin-top:20px' }, h('h2', { style: 'margin-top:0' }, t('Add a library')),
      formRow(t('Name'), name), formRow(t('Kind'), type), formRow(t('Language of titles and plots'), newLang), formRow(t('Folders'), paths, null, true),
      h('div', { class: 'actions' }, h('button', { onclick: () => folderPicker(p => { paths.value = (paths.value.trim() ? paths.value.trim() + '\n' : '') + p; }) }, t('Browse folders…')),
        h('button', { class: 'primary', onclick: async () => {
          try { await api('POST', '/library/sections', { name: name.value, type: type.value, paths: paths.value, language: newLang.value }); toast(t('Library added; scanning')); name.value = ''; paths.value = ''; refreshList(2); pollActivity(); }
          catch (e) { toast(e.message, 5000); }
        } }, t('Add library')))),
    await importPanel());
  if (secs.some(s => s.scanning)) refreshList();
}

// ---------- admin: import from the old server
async function importPanel() {
  const st = await GET('/api/import/old-server');
  const where = h('input', { value: st.detected || '', placeholder: t('Data folder of the old server, or its library .db file'), style: 'width:100%;max-width:620px' });
  const out = h('div', {});
  const lastBox = h('div', {});
  const showLast = r => {
    if (!r) return lastBox.replaceChildren();
    const f = r.files || {}, w = r.watched || {}, p = r.playlists || {};
    lastBox.replaceChildren(h('div', { class: 'panel', style: 'background:var(--panel2)' },
      h('b', {}, t('Last import')), ' ', h('span', { class: 'dim' }, fmtDateTime(r.finished_at)),
      h('div', { class: 'kv', style: 'margin-top:8px' },
        h('div', {}, t('Files found here')), h('div', {}, t('{n} of {all}', { n: f.found_here, all: f.in_old_database })),
        h('div', {}, t('Libraries added')), h('div', {}, (r.libraries_created || []).map(l => l.name).join(', ') || t('none (existing ones used)')),
        h('div', {}, t('Watched / resume points / ratings')), h('div', {}, `${w.watched} / ${w.resume_points} / ${w.ratings}`),
        h('div', {}, t('Playlists')), h('div', {}, [...(p.created || []), ...(p.updated || [])].join(', ') || t('none')),
        h('div', {}, t('Credits markers')), h('div', {}, String(r.credits_markers)),
        h('div', {}, t('Titles and plots')), h('div', {}, t('{n} items', { n: r.details })),
        (p.smart_not_imported || []).length ? [h('div', {}, t('Not imported')), h('div', {}, t('smart playlists (their rules work differently here): ') + p.smart_not_imported.join(', '))] : null,
        (w.accounts_skipped || []).length ? [h('div', {}, t('Accounts skipped')), h('div', {}, w.accounts_skipped.join(', '))] : null),
      f.not_found ? h('details', { style: 'margin-top:8px' }, h('summary', { class: 'dim' }, t('{n} files were not found here — check the folder translation', { n: f.not_found })),
        h('pre', { style: 'white-space:pre-wrap;font-size:12px' }, (f.not_found_examples || []).join('\n'))) : null));
  };
  showLast(st.last);
  const look = async () => {
    out.replaceChildren(h('p', { class: 'dim' }, t('Reading the old database (a copy; the old server is not changed)…')));
    let pv, users;
    try { [pv, users] = await Promise.all([GET('/api/import/old-server/preview?db=' + encodeURIComponent(where.value)), GET('/api/users')]); }
    catch (e) { return out.replaceChildren(h('p', { class: 'err' }, e.message)); }
    const maps = [];
    const libs = pv.libraries.map(l => h('div', { style: 'margin:10px 0' },
      h('b', {}, l.name), ' ', h('span', { class: 'tag' }, sectionKind(l.type)), l.imported ? null : h('span', { class: 'dim' }, ' — ' + t('not imported (only movies and TV shows)')),
      l.imported ? l.folders.map(f => {
        const to = h('input', { value: f.suggested || f.old, style: 'flex:1;min-width:220px' });
        maps.push({ from: f.old, to });
        return h('div', { style: 'display:flex;gap:8px;align-items:center;margin-top:6px;flex-wrap:wrap' },
          h('code', { style: 'flex:1;min-width:220px;word-break:break-all' }, f.old), '→', to,
          h('button', { class: 'small', onclick: () => folderPicker(p => { to.value = p; }) }, t('Browse…')));
      }) : null));
    const acc = {};
    const accounts = pv.accounts.map(a => {
      const sel = h('select', {}, h('option', { value: '0' }, t('skip')), users.users.map(u => h('option', { value: u.id, selected: a.owner ? u.id === S.me.id : a.suggested_user === u.id }, u.username)));
      acc[a.id] = sel;
      return h('tr', {}, h('td', {}, a.name, a.owner ? h('span', { class: 'badge' }, t('owner')) : ''), h('td', { class: 'dim' }, t('{n} watched', { n: a.watched })), h('td', {}, '→ ', sel));
    });
    const opt = {};
    const box = (k, label) => h('label', {}, (opt[k] = h('input', { type: 'checkbox', checked: true })), label);
    const c = pv.counts;
    out.replaceChildren(
      h('p', {}, t('Found: {movies} movies, {shows} shows, {episodes} episodes, {watched} watched, {resume} resume points, {ratings} ratings, {playlists} playlists (+{smart} smart), {markers} credits markers.',
        { movies: c.movies, shows: c.shows, episodes: c.episodes, watched: c.watched_states, resume: c.resume_points, ratings: c.ratings, playlists: c.playlists, smart: c.smart_playlists, markers: c.credits_markers })),
      h('h3', {}, t('Folders')), h('p', { class: 'dim' }, t('Where each folder of the old server is on this server. In Docker your video share is usually under /media.')), libs,
      h('h3', {}, t('People')), h('p', { class: 'dim' }, t('Whose watched state, ratings and playlists go to which user here. Invite the others first to bring theirs over, then import again.')),
      h('table', {}, accounts),
      h('h3', {}, t('What to bring over')),
      h('div', { class: 'checks' }, box('libraries', t('Libraries (added and scanned)')), box('watched', t('Watched, resume points and ratings')), box('playlists', t('Playlists')), box('markers', t('Credits markers')), box('details', t('Titles, plots and genres (kept; online details add posters and cast)'))),
      h('div', { class: 'actions' }, h('button', { class: 'primary', onclick: async e => {
        const body = { db: pv.db, path_map: maps.map(m => ({ from: m.from, to: m.to.value.trim() })), accounts: {} };
        for (const [id, s] of Object.entries(acc)) if (s.value !== '0') body.accounts[id] = Number(s.value);
        for (const [k, el] of Object.entries(opt)) body[k] = el.checked;
        try { await api('POST', '/api/import/old-server', body); e.target.disabled = true; toast(t('Import started — progress is shown at the top'), 5000); pollActivity(); waitDone(); }
        catch (er) { toast(er.message, 6000); }
      } }, t('Import'))),
      pv.warnings.length ? h('p', { class: 'dim', style: 'font-size:12px' }, t('Notes: ') + pv.warnings.map(w => t(w)).join('; ')) : null);
  };
  const waitDone = async () => {
    for (let i = 0; i < 2400; i++) {
      await new Promise(r => setTimeout(r, 1500));
      const s = await GET('/api/import/old-server').catch(() => null);
      if (s && !s.running) { showLast(s.last); toast(t('Import finished')); S.sections = (await GET('/library/sections')).MediaContainer.Directory; drawChrome(); return; }
    }
  };
  if (st.running) waitDone();
  return h('div', { class: 'panel', style: 'margin-top:20px' },
    h('h2', { style: 'margin-top:0' }, t('Import from your old server')),
    h('p', { class: 'dim' }, t('Brings over libraries, everyone’s watched state, resume points and ratings, playlists, credits markers and titles/plots from the server this one replaces. It reads a copy of its library database ('),
      h('code', {}, 'com.plexapp.plugins.library.db'), t('); the old server keeps working. You can run it again any time — nothing is counted twice.')),
    formRow(t('Old server’s data'), where, st.detected ? t('Found automatically.') : t('Not found automatically. On a Synology with Docker, add the old server’s data folder to docker-compose.yml (see the README) — it then appears under /oldserver.')),
    h('div', { class: 'actions' }, h('button', { onclick: look }, t('Look inside'))),
    out, lastBox);
}

// A library's folders: one row each, typed or picked. values() = the folders given.
function folderRows(paths) {
  const rows = h('div', {});
  const row = p => {
    const inp = h('input', { value: p, placeholder: '/media/Movies', style: 'flex:1;min-width:200px' });
    const r = h('div', { class: 'folder-row', style: 'display:flex;gap:8px;align-items:center;margin-bottom:8px;flex-wrap:wrap' }, inp,
      h('button', { class: 'small', onclick: () => folderPicker(x => { inp.value = x; }) }, t('Browse…')),
      h('button', { class: 'small danger', onclick: () => r.remove() }, t('Remove')));
    rows.append(r);
    return inp;
  };
  paths.forEach(row);
  const el = h('div', { style: 'width:100%' }, rows,
    h('div', { class: 'actions', style: 'margin-top:0' },
      h('button', { onclick: () => folderPicker(p => row(p)) }, t('Add folder…')),
      h('button', { onclick: () => row('').focus() }, t('Type a folder'))));
  return { el, values: () => [...rows.querySelectorAll('input')].map(i => i.value.trim()).filter(Boolean) };
}
// The folders of one library, saved together.
function foldersDialog(s) {
  const folders = folderRows(s.paths);
  const box = modal(t('Folders of {name}', { name: s.title }), [
    h('p', { class: 'dim' }, t('The library is scanned again after saving. Removing a folder takes its videos out of the library; the files stay on disk.')),
    folders.el,
    h('div', { class: 'actions', style: 'margin-top:14px' },
      h('button', { class: 'primary', onclick: async () => {
        const paths = folders.values();
        if (!paths.length) return toast(t('Give at least one folder in paths'));
        try { await api('PUT', '/library/sections/' + s.id, { paths }); toast(t('Saved')); box.remove(); librariesPage(); pollActivity(); }
        catch (e) { toast(e.message, 5000); }
      } }, t('Save')),
      h('button', { onclick: () => box.remove() }, t('Cancel')))]);
}

function folderPicker(done) {
  const list = h('div', { class: 'list', style: 'max-height:50vh;overflow:auto' });
  const here = h('div', { class: 'dim', style: 'margin-bottom:8px;word-break:break-all' });
  let cur = '';
  const box = modal(t('Pick a folder'), [here, list, h('div', { class: 'actions' }, h('button', { class: 'primary', onclick: () => { if (cur) { done(cur); box.remove(); } } }, t('Use this folder')), h('button', { onclick: () => box.remove() }, t('Cancel')))]);
  const open = async p => {
    try {
      const r = await GET('/api/browse?path=' + encodeURIComponent(p));
      cur = r.path; here.textContent = (r.path || t('Drives')) + (r.videos_here ? ' — ' + t('{n} videos here', { n: r.videos_here }) : '');
      list.replaceChildren(...(r.parent !== null && r.parent !== undefined ? [h('div', { class: 'li', style: 'cursor:pointer', onclick: () => open(r.parent) }, '⬆ ' + t('Up'))] : []),
        ...r.dirs.map(d => h('div', { class: 'li', style: 'cursor:pointer', onclick: () => open(d.path) }, '📁 ' + d.name)));
    } catch (e) { toast(e.message); }
  };
  open('');
}

// ---------- admin: users and invites
async function usersPage() {
  markNav('users');
  const [users, invites] = await Promise.all([GET('/api/users'), GET('/api/invites')]);
  const secName = id => (S.sections.find(s => s.id === id) || {}).title || '#' + id;
  const email = h('input', { type: 'email', placeholder: t('friend@example.com'), style: 'width:100%;max-width:420px' });
  const note = h('input', { placeholder: t('A short message (optional)'), style: 'width:100%;max-width:420px' });
  const days = h('input', { type: 'number', min: 1, max: 365, value: 7, style: 'width:90px' });
  const asAdmin = h('input', { type: 'checkbox' });
  const allLibs = h('input', { type: 'checkbox', checked: true });
  const libChecks = S.sections.map(s => ({ s, el: h('input', { type: 'checkbox', checked: true }) }));
  const result = h('div');
  const copy = url => { if (navigator.clipboard) navigator.clipboard.writeText(url); toast(t('Link copied')); };
  set(h('h1', {}, t('Users & invites')),
    h('div', { class: 'panel' }, h('h2', { style: 'margin-top:0' }, t('Invite someone')),
      h('p', { class: 'dim' }, t('You get a link to send them (by email, chat …). They open it, pick a user name and password, and can watch the libraries you tick.')),
      formRow(t('Their email'), email), formRow(t('Message'), note),
      formRow(t('Link works for (days)'), days),
      formRow(t('Libraries they may watch'), h('div', { class: 'checks' }, h('label', {}, allLibs, h('b', {}, t('All, including future ones'))), libChecks.map(c => h('label', {}, c.el, c.s.title)))),
      formRow(t('Make them an admin'), asAdmin),
      h('div', { class: 'actions' }, h('button', { class: 'primary', onclick: async () => {
        const body = { email: email.value, note: note.value, days: Number(days.value) || 7, admin: asAdmin.checked };
        if (!allLibs.checked) body.allowed_sections = libChecks.filter(c => c.el.checked).map(c => c.s.id);
        const r = await api('POST', '/api/invites', body);
        const mail = `mailto:${encodeURIComponent(r.email)}?subject=${encodeURIComponent(t('Invitation to {server}', { server: S.status ? S.status.name : 'Lichtspiel' }))}&body=${encodeURIComponent((r.note ? r.note + '\n\n' : '') + t('Join here: {url}', { url: r.url }))}`;
        result.replaceChildren(h('div', { class: 'panel', style: 'background:var(--panel2)' }, h('div', { class: 'ok' }, t('Invite ready. Send this link:')),
          h('input', { value: r.url, style: 'width:100%;margin:8px 0', onclick: e => e.target.select() }),
          h('button', { onclick: () => copy(r.url) }, t('Copy link')), ' ', r.email ? h('a', { href: mail }, h('button', {}, t('Open in email'))) : null));
      } }, t('Create invite link'))), result),
    h('h2', {}, t('People')),
    h('table', {}, h('tr', {}, h('th', {}, t('User')), h('th', {}, t('Libraries')), h('th', {}, t('Admin')), h('th', {}, t('May convert')), h('th', {}, t('May delete files')), h('th', {}, t('May download')), h('th', {}, '')),
      users.users.map(u => h('tr', {},
        h('td', {}, h('b', {}, u.username), h('div', { class: 'dim' }, u.email || ''), h('div', { class: 'dim', style: 'font-size:12px' }, t('joined {date}', { date: fmtDate(u.created_at) }))),
        h('td', {}, u.admin ? t('all (admin)') : u.allowed_sections === null ? t('all') : u.allowed_sections.map(secName).join(', ') || t('none'), ' ',
          u.admin ? null : h('button', { class: 'small', onclick: () => accessEditor(u) }, t('Change'))),
        h('td', {}, h('input', { type: 'checkbox', checked: u.admin, disabled: u.id === S.me.id, onchange: async e => { try { await api('PATCH', '/api/users/' + u.id, { admin: e.target.checked }); } catch (x) { toast(x.message); } usersPage(); } })),
        h('td', {}, h('input', { type: 'checkbox', checked: u.allow_transcode, title: t('Allow converting video on the fly'), onchange: async e => { await api('PATCH', '/api/users/' + u.id, { allow_transcode: e.target.checked }); toast(t('Saved')); } })),
        h('td', {}, h('input', { type: 'checkbox', checked: u.admin || u.allow_delete, disabled: u.admin,
          title: u.admin ? t('Admins may always delete files (when media deletion is allowed in the settings)') : t('May delete movie and episode files from disk in the libraries they can see'),
          onchange: async e => { await api('PATCH', '/api/users/' + u.id, { allow_delete: e.target.checked }); toast(t('Saved')); } })),
        h('td', {}, h('input', { type: 'checkbox', checked: u.allow_download, disabled: u.admin,
          title: u.admin ? t('Admins may always download') : t('May download movies and episodes, as they are or converted to a smaller quality'),
          onchange: async e => { await api('PATCH', '/api/users/' + u.id, { allow_download: e.target.checked }); toast(t('Saved')); } })),
        h('td', {}, u.id === S.me.id ? h('span', { class: 'dim' }, t('you')) : [
          h('button', { class: 'small', onclick: async () => { const pw = await ask(t('New password for {name}', { name: u.username }), { password: true }); if (pw) { try { await api('PATCH', '/api/users/' + u.id, { password: pw }); toast(t('Password changed')); } catch (x) { toast(x.message); } } } }, t('Password')), ' ',
          h('button', { class: 'small danger', onclick: async () => { if (await sure(t('Remove {name}?', { name: u.username }), { ok: t('Remove'), danger: true })) { await api('DELETE', '/api/users/' + u.id); usersPage(); } } }, t('Remove'))])))),
    h('h2', {}, t('Invites')),
    invites.invites.length ? h('table', {}, h('tr', {}, h('th', {}, t('For')), h('th', {}, t('Status')), h('th', {}, t('Libraries')), h('th', {}, t('Link')), h('th', {}, '')),
      invites.invites.map(i => h('tr', {},
        h('td', {}, i.email || h('span', { class: 'dim' }, t('anyone with the link')), i.note ? h('div', { class: 'dim' }, i.note) : null),
        h('td', {}, i.status === 'used' ? t('used by {name}', { name: i.used_by }) : i.status === 'open' ? t('open until {date}', { date: fmtDate(i.expires_at) }) : t('expired')),
        h('td', {}, i.allowed_sections ? i.allowed_sections.map(secName).join(', ') : t('all')),
        h('td', {}, i.status === 'open' ? h('button', { class: 'small', onclick: () => copy(i.url) }, t('Copy link')) : ''),
        h('td', {}, h('button', { class: 'small danger', onclick: async () => { await api('DELETE', '/api/invites/' + i.code); usersPage(); } }, i.status === 'open' ? t('Cancel') : t('Delete')))))) : h('p', { class: 'dim' }, t('No invites yet.')));
}

function accessEditor(u) {
  const all = h('input', { type: 'checkbox', checked: u.allowed_sections === null });
  const checks = S.sections.map(s => ({ s, el: h('input', { type: 'checkbox', checked: u.allowed_sections === null || u.allowed_sections.includes(s.id) }) }));
  const box = modal(t('Libraries for {name}', { name: u.username }), [h('div', { class: 'checks' }, h('label', {}, all, h('b', {}, t('All, including future ones'))), checks.map(c => h('label', {}, c.el, c.s.title))),
    h('div', { class: 'actions' }, h('button', { class: 'primary', onclick: async () => {
      await api('PATCH', '/api/users/' + u.id, { allowed_sections: all.checked ? null : checks.filter(c => c.el.checked).map(c => c.s.id) }); box.remove(); usersPage();
    } }, t('Save')), h('button', { onclick: () => box.remove() }, t('Cancel')))]);
}

// ---------- admin: plugins
async function pluginsPage() {
  markNav('plugins');
  const [r, log] = await Promise.all([GET('/api/plugins'), GET('/api/plugins/log')]);
  set(h('h1', {}, t('Plugins')),
    h('p', { class: 'dim' }, t('Plugins add features: fill in movie details, send notifications, react to playback, add their own API actions. Put each plugin in its own folder inside '), h('code', {}, r.folder), t(' with a '), h('code', {}, 'plugin.json'), t(' file, then press Reload. Any language works (Python, shell, Node …).')),
    h('div', { class: 'actions' }, h('button', { onclick: async () => { const x = await api('POST', '/api/plugins/reload'); toast(t('{n} plugin(s) found', { n: x.count })); pluginsPage(); } }, t('Reload plugins'))),
    r.errors.length ? h('div', { class: 'panel', style: 'color:var(--red)' }, r.errors.map(e => h('div', {}, e))) : null,
    r.plugins.length ? r.plugins.map(p => {
      const ta = h('textarea', {}, JSON.stringify(p.settings, null, 2));
      return h('div', { class: 'panel' },
        h('div', { style: 'display:flex;gap:12px;align-items:center' }, h('b', { style: 'font-size:17px' }, p.name), h('span', { class: 'dim' }, 'v' + (p.version || '?')), p.author ? h('span', { class: 'dim' }, t('by {name}', { name: p.author })) : null, h('span', { style: 'flex:1' }),
          h('span', { class: p.enabled ? 'ok' : 'dim' }, p.enabled ? t('on') : t('off')),
          h('button', { class: p.enabled ? '' : 'primary', onclick: async () => { await api('POST', `/api/plugins/${p.id}/${p.enabled ? 'disable' : 'enable'}`); pluginsPage(); } }, p.enabled ? t('Switch off') : t('Switch on'))),
        h('p', {}, p.description), h('div', {}, t('Listens to: '), p.hooks.map(x => h('span', { class: 'tag' }, x))),
        formRow(t('Settings'), ta, null, true),
        h('div', { class: 'actions' }, h('button', { onclick: async () => { try { await api('PUT', `/api/plugins/${p.id}/settings`, JSON.parse(ta.value)); toast(t('Saved')); } catch (e) { toast(e.message); } } }, t('Save settings'))));
    }) : h('div', { class: 'empty' }, t('No plugins installed yet.')),
    h('h2', {}, t('Recent plugin activity')),
    log.log.length ? h('table', {}, log.log.slice(0, 40).map(l => h('tr', {}, h('td', { class: 'dim' }, new Date(l.at * 1000).toLocaleTimeString(I18N.lang)), h('td', {}, l.plugin), h('td', {}, l.event), h('td', { class: l.ok ? 'ok' : 'err' }, l.ok ? t('ok') : t('failed')), h('td', { class: 'dim', style: 'word-break:break-all' }, l.detail)))) : h('p', { class: 'dim' }, t('Nothing yet.')),
    h('h2', {}, t('Events a plugin can listen to')), h('p', {}, r.events.map(e => h('code', { style: 'margin-right:6px' }, e))));
}

// ---------- admin: settings
// Every user can pick the app's language and change their password; the server settings are for admins.
function accountPanel(dataDir) {
  return h('div', { class: 'panel' }, h('h2', { style: 'margin-top:0' }, t('Your account')),
    formRow(t('Language of this app'), langSelect(), t('Saved in this browser.')),
    h('div', { style: 'margin-top:14px' }, h('button', { onclick: async () => { const old = await ask(t('Current password'), { password: true }); if (!old) return; const nw = await ask(t('New password (at least 6 characters)'), { password: true }); if (!nw) return; try { await api('POST', '/api/me/password', { old, new: nw }); toast(t('Password changed')); } catch (e) { toast(e.message); } } }, t('Change my password'))),
    dataDir ? h('p', { class: 'dim' }, t('Data folder on the server: '), h('code', {}, dataDir)) : null);
}

async function settingsPage() {
  markNav('settings');
  if (!S.me.admin) return set(h('h1', {}, t('Settings')), accountPanel());
  const [r, tc] = await Promise.all([GET('/api/settings'), GET('/api/transcoder')]);
  const s = r.settings;
  const f = {};
  const inp = (k, label, type, help) => formRow(label, (f[k] = h('input', { type: type || 'text', value: s[k], style: 'width:100%;max-width:420px' })), help);
  const sel = (k, label, opts, help) => formRow(label, (f[k] = h('select', {}, opts.map(([v, l]) => h('option', { value: v, selected: String(s[k]) === String(v) }, l)))), help);
  const chk = (k, label, help) => formRow(label, (f[k] = h('input', { type: 'checkbox', checked: !!s[k] })), help);
  set(h('h1', {}, t('Settings')),
    h('div', { class: 'panel' }, h('h2', { style: 'margin-top:0' }, t('Server')),
      inp('server_name', t('Server name')),
      inp('port', t('Port'), 'number', t('Default 8484. Takes effect after a restart; right now the server listens on {port}. With Docker, change the port mapping in docker-compose.yml instead.', { port: r.port_in_use })),
      inp('public_url', t('Address used in invite links'), 'text', t('e.g. http://my-nas.example.com:8484 — leave empty to use the address you opened this page with')),
      chk('scheduled_scans_enabled', t('Scan my library periodically'), t('Off by default. Turn it on so new files show up without pressing "Scan".')),
      sel('scan_interval_minutes', t('Library scan interval'), r.scan_intervals.map(x => [x.minutes, t(x.label)])),
      sel('watched_threshold_percent', t('Video played threshold'), r.played_thresholds.map(p => [p, t('{n}%', { n: p })]), t('Set the progress percentage for video playback at which point the video will be marked as played.')),
      chk('watch_folders', t('Scan my library automatically'), t('Your library will be updated automatically when files are added to, changed in or removed from library folders (checked every 15 seconds; a file is added once copying has finished).')),
      sel('credits_markers_behaviour', t('Generate credits video markers'), r.credits_marker_choices.map(c => [c.value, t(c.label)]), t('Detects movie and episode end credits: from chapters named "Credits" in the file, otherwise by looking at the end of the video. Can also be switched off per library; the Libraries page has a "Detect credits" button to run it now.')),
      chk('credits_from_picture', t('Look at the picture to find credits'), t('When a file has no "Credits" chapter, the last part of the video is checked for the typical credits look (dark screen with text). Needs ffmpeg.')),
      sel('played_at_behaviour', t('Mark videos as watched'), r.played_at_choices.map(c => [c.value, t(c.label)]), t('Credits markers come from chapters named "Credits" / "Abspann" in the file, or are set by hand on the movie or episode page. Without markers the threshold is used.')),
      inp('continue_watching_weeks', t('Weeks to consider for Continue Watching'), 'number'), inp('continue_watching_limit', t('Maximum Continue Watching items'), 'number'),
      chk('continue_watching_include_premieres', t('Include season premieres in Continue Watching'), t('New season premieres always appear no matter how many weeks have passed since watching.')),
      inp('invite_default_days', t('Invite links last (days)'), 'number'),
      chk('empty_trash_automatically', t('Empty trash automatically after every scan'), t('Files gone from disk leave the library right away. Off: they stay as unavailable (with their watched state and details) until you choose "Empty trash" in the library’s menu. A library folder that cannot be read never empties anything.')),
      chk('allow_media_deletion', t('Allow media deletion'), t('Admins, and people you allow under Users & invites, may delete movie and episode files from disk with "Delete" in the "⋯" menu.')),
      inp('download_keep_days', t('Keep converted downloads (days)'), 'number', t('Copies converted for “Download…” are kept in the server’s data folder (downloads) and deleted this many days after they were made. 0 = until they are removed from the Downloads page.'))),
    h('div', { class: 'panel' }, h('h2', { style: 'margin-top:0' }, t('Movie and show details')),
      chk('metadata_enabled', t('Download plot, cast, posters and episode titles from the internet')),
      sel('metadata_language', t('Language of titles, plots and episode names'), r.languages.map(l => [l.code, l.name]), t('Used by every library that is set to "Same as server" on the Libraries page (each library can have its own). Changing it downloads the details again in the background — watch the progress at the top of the page. Show episode names are only translated with a TMDB key.')),
      chk('imdb_ratings_file', t('Show IMDb ratings from IMDb’s ratings file'), t('Downloads IMDb’s free ratings list (about 9 MB) once a week. Rotten Tomatoes and Metacritic scores come from Wikidata; the TMDB score needs a TMDB key.')),
      formRow('', h('button', { class: 'small', onclick: async () => {
        try { await api('POST', '/api/imdb-ratings/refresh'); toast(t('Downloading IMDb ratings — progress is shown at the top'), 5000); pollActivity(); } catch (e) { toast(e.message, 6000); }
      } }, t('Update IMDb ratings now')), t('Downloads the ratings file again right away and updates every movie and show that has an IMDb id.')),
      inp('tmdb_api_key', t('TMDB API key (optional)'), 'text', t('Without a key, movies come from Wikipedia/Wikidata and TV shows from TVmaze. A free key from themoviedb.org gives better matches, cast photos and translated plots.')),
      inp('omdb_api_key', t('OMDb API key for IMDb plots (optional)'), 'text', t('With a free key from omdbapi.com, movie and show plots are taken from IMDb (in English) instead of Wikipedia. Press Refresh details on a movie, or change the language, to load them for movies you already have.'))),
    h('div', { class: 'panel' }, h('h2', { style: 'margin-top:0' }, t('Transcoder')),
      h('p', { class: tc.ffmpeg_found ? 'ok' : 'err' }, tc.ffmpeg_found ? t('ffmpeg found: {version}', { version: tc.ffmpeg_version }) : t('ffmpeg was not found. Install it (the Docker image includes it) or enter its path below; without it only browser-friendly files play.')),
      tc.hardware_encoders.length ? h('p', { class: 'dim' }, t('Hardware encoders in this ffmpeg: ') + tc.hardware_encoders.join(', ') + (tc.render_device ? ' · ' + t('/dev/dri/renderD128 present') : '')) : null,
      h('p', { class: 'dim' }, t('Converting with: ') + (tc.hw_in_use === 'none' ? t('the processor (software)') : tc.hw_in_use)),
      sel('transcoder_quality', t('Transcoder quality'), [['automatic', t('Automatic')], ['speed', t('Prefer higher speed encoding')], ['quality', t('Prefer higher quality encoding')], ['max', t('Make my CPU hurt')]], t('Applies to Intel Quick Sync and NVIDIA hardware encoding.')),
      sel('hw_accel', t('Hardware acceleration'), [['auto', t('Automatic (use the graphics chip when it works)')], ['none', t('Off (software)')], ['vaapi', t('Intel/AMD VAAPI (most Synology Plus models)')], ['qsv', 'Intel Quick Sync'], ['nvenc', 'NVIDIA']]),
      chk('hw_constant_quality', t('Graphics chip: convert by quality instead of bitrate'), t('Off: the graphics chip gets a fixed bitrate (safe on every model). On: it gets a quality number from 15 (best) to 25, like the software encoder does. Try it if hardware-converted video looks blocky.')),
      sel('h264_preset', t('Software encoder speed (x264 preset)'), r.h264_presets.map(p => [p, p])),
      inp('h264_minimum_crf', t('Best allowed x264 quality (minimum CRF)'), 'number', t('Lower = better picture and bigger stream. The quality the player asks for is mapped between this and 24.')),
      inp('max_transcodes', t('Maximum simultaneous video transcodes (0 = no limit)'), 'number'),
      inp('hls_segment_seconds', t('HLS segment length (seconds)'), 'number'), inp('segmented_transcoder_timeout_seconds', t('Segmented transcoder timeout (seconds)'), 'number'),
      inp('ffmpeg', t('ffmpeg program')), inp('ffprobe', t('ffprobe program'))),
    h('div', { class: 'actions' }, h('button', { class: 'primary', onclick: async () => {
      const body = {};
      for (const [k, el] of Object.entries(f)) body[k] = typeof s[k] === 'boolean' ? el.checked : typeof s[k] === 'number' ? Number(el.value) : el.value;
      try {
        const x = await api('PUT', '/api/settings', body);
        toast([t('Saved'), x.refetching ? t('details for {n} movies and shows are being downloaded again in the new language', { n: x.refetching }) : '', x.restart_needed ? t(x.restart_needed) : ''].filter(Boolean).join(' — '), x.restart_needed || x.refetching ? 8000 : 2600);
        boot();
      } catch (e) { toast(e.message, 5000); }
    } }, t('Save settings'))),
    accountPanel(r.data_dir));
}

// ---------- API page
async function apiPage() {
  markNav('api');
  const d = await GET('/api/docs');
  const shown = h('code', {}, '••••••••');
  const search = h('input', { type: 'search', placeholder: t('Find an address…'), style: 'width:260px' });
  const groups = d.groups.map(g => {
    const rows = g.routes.map(r => {
      const tr = h('tr', {}, h('td', { style: 'width:70px' }, h('code', { class: 'm-' + r.method.toLowerCase() }, r.method)), h('td', { style: 'width:36%' }, h('code', {}, r.path)), h('td', { class: 'dim' }, r.description));
      tr.dataset.text = (r.method + ' ' + r.path + ' ' + r.description).toLowerCase();
      return tr;
    });
    const box = h('details', { class: 'panel apigroup', id: 'g-' + g.name.replace(/\W+/g, '-') },
      h('summary', {}, h('b', {}, g.name), h('span', { class: 'dim' }, ` · ${g.routes.length}`)),
      h('table', {}, rows));
    return { box, rows };
  });
  search.oninput = () => {
    const q = search.value.trim().toLowerCase();
    for (const g of groups) {
      let n = 0;
      for (const r of g.rows) { const hit = !q || r.dataset.text.includes(q); r.style.display = hit ? '' : 'none'; n += hit; }
      g.box.style.display = n ? '' : 'none';
      if (q) g.box.open = true;
    }
  };
  const tokens = h('div', {});
  const loadTokens = async () => {
    const tk = await GET('/api/tokens');
    tokens.replaceChildren(h('table', {}, tk.tokens.map(k => h('tr', {},
      h('td', {}, h('code', {}, k.id)), h('td', {}, k.device || t('Web app'), k.current ? h('span', { class: 'badge' }, t('this one')) : ''),
      h('td', { class: 'dim' }, fmtDateTime(k.created_at)),
      h('td', {}, k.current ? '' : h('button', { class: 'small', onclick: async () => { await api('DELETE', '/api/tokens/' + k.id); toast(t('Token revoked')); loadTokens(); } }, t('Revoke')))))));
  };
  const rotate = async all => {
    if (!await sure(all ? t('Make a new token and sign out every other device and script?') : t('Make a new token? Scripts using the old one stop working.'))) return;
    const r = await api('POST', '/api/tokens/rotate', { all });
    S.token = r.token; localStorage.setItem('token', r.token); shown.textContent = '••••••••';
    toast(all ? t('New token made; {n} other sign-in(s) ended', { n: r.revoked - 1 }) : t('New token made; the old one no longer works'));
    loadTokens();
  };
  set(h('h1', {}, t('API')),
    h('p', {}, t('Everything this app does goes through this JSON API, so other apps and scripts can do the same. '), d.auth, '.'),
    h('div', { class: 'panel' }, t('Your token: '), shown, ' ',
      h('button', { class: 'small', onclick: () => { shown.textContent = S.token; } }, t('Show')), ' ',
      h('button', { class: 'small', onclick: () => rotate(false) }, t('Rotate token')), ' ',
      h('button', { class: 'small', onclick: () => rotate(true) }, t('Rotate and sign out everywhere else')), ' ',
      h('button', { class: 'small', onclick: async () => { const n = await ask(t('Name for the new token (e.g. "backup script")')); if (n === null) return; const r = await api('POST', '/api/tokens', { device: n }); await ask(t('Copy the new token now; it is not shown again:'), { value: r.token, ok: t('Done') }); loadTokens(); } }, t('New token for a script')),
      h('p', { class: 'dim' }, t('Example: '), h('code', {}, `curl -H "X-Token: <token>" ${location.origin}/library/sections`)),
      tokens),
    h('div', { class: 'panel' }, h('b', {}, t('Filters')), h('p', { class: 'dim' }, t('Browsing, playlist contents and smart playlists share the same fields: '), S.filters.fields.map(f => h('code', { style: 'margin-right:5px' }, f.field)), t('. In addresses: '), Object.entries(S.filters.url_shortcuts).map(([k, v]) => h('span', {}, h('code', {}, k), ` ${v}; `)))),
    h('div', { style: 'display:flex;gap:8px;flex-wrap:wrap;align-items:center;margin:10px 0' }, search,
      h('button', { class: 'small', onclick: () => groups.forEach(g => g.box.open = true) }, t('Open all')),
      h('button', { class: 'small', onclick: () => groups.forEach(g => g.box.open = false) }, t('Close all'))),
    groups.map(g => g.box));
  loadTokens();
}

// ---------- an actor and everything they are in
async function personPage(key, qs) {
  markNav('');
  const name = new URLSearchParams(qs || '').get('name') || '';
  const r = await GET(`/library/people/${encodeURIComponent(key)}?name=${encodeURIComponent(name)}`);
  const initials = () => h('div', { class: 'initials', style: 'width:160px;height:220px;font-size:42px' }, (r.name || '?').split(' ').map(w => w[0]).slice(0, 2).join(''));
  const src = r.thumb ? withTok(r.thumb) : r.photo;
  const pic = src ? h('img', { src, alt: '', referrerpolicy: 'no-referrer', style: 'width:160px;height:220px;object-fit:cover;border-radius:8px', onerror: e => e.target.replaceWith(initials()) }) : initials();
  set(h('div', { class: 'detail', style: 'margin-bottom:24px' }, h('div', {}, pic),
      h('div', { class: 'info' }, h('h1', {}, r.name || t('Unknown')), h('p', { class: 'dim' }, r.size === 1 ? t('In 1 movie or show in your libraries') : t('In {n} movies and shows in your libraries', { n: r.size })))),
    r.Metadata.length ? h('div', { class: 'grid' }, r.Metadata.map(it => h('div', {}, posterCard(it), it.role ? h('div', { class: 'dim', style: 'font-size:12px;margin-top:2px' }, t('as {role}', { role: it.role })) : null)))
      : h('div', { class: 'empty' }, t('Nothing in your libraries.')));
}

// ---------- the "…" menu on posters and on a movie's page
// The item menu: watch together, share, queue, add to …, watched,
// and for the owner refresh, analyze, fix match, optimize, access; then
// download, play history, report a problem and media info.
const PLAYABLE = ['movie', 'episode', 'video', 'clip'];
const isPlayable = it => PLAYABLE.includes(it.type);
// The whole item (lists leave out the streams and the online details).
const fullItem = async it => (it.Media && it.Media[0].streams) || it._full ? it : Object.assign(meta(await GET('/library/metadata/' + it.id))[0], { _full: true });
// Episodes of a show or season, or the video itself.
const leavesOf = async it => isPlayable(it) ? [it] : meta(await GET(`/library/metadata/${it.id}/allLeaves`));

function closeMenus() { document.querySelectorAll('.menu').forEach(m => m.remove()); }
document.addEventListener('mousedown', e => { if (!e.target.closest('.menu')) closeMenus(); }, true);
window.addEventListener('hashchange', closeMenus);
window.addEventListener('resize', closeMenus);
document.addEventListener('keydown', e => { if (e.key === 'Escape' && document.querySelector('.menu')) { e.stopImmediatePropagation(); closeMenus(); } }, true);

// A list of entries next to `anchor`. Entries: {label, run, sub: async () => entries, danger} or '-' (a line).
function openMenu(anchor, entries, parent) {
  if (!parent) closeMenus();
  const box = h('div', { class: 'menu', role: 'menu' });
  let subOpen = null;
  const closeSub = () => { if (subOpen) { subOpen.remove(); subOpen = null; } };
  for (const e of entries.filter(Boolean)) {
    if (e === '-') { if (box.lastChild && !box.lastChild.classList.contains('sep')) box.append(h('div', { class: 'sep' })); continue; }
    const row = h('button', { class: 'mi' + (e.sub ? ' has-sub' : '') + (e.danger ? ' danger' : ''), role: 'menuitem', disabled: e.disabled }, h('span', {}, e.label), e.sub ? h('span', { class: 'dim' }, '›') : null);
    const openSub = async () => {
      if (subOpen && subOpen._from === row) return;
      closeSub();
      let kids;
      try { kids = await e.sub(); } catch (x) { return toast(x.message, 6000); }
      if (!box.isConnected) return;
      subOpen = openMenu(row, kids, box); subOpen._from = row;
    };
    if (e.sub) { row.onmouseenter = openSub; row.onclick = openSub; }
    else {
      row.onmouseenter = closeSub;
      row.onclick = async () => { closeMenus(); try { await e.run(); } catch (x) { toast(x.message, 6000); } };
    }
    box.append(row);
  }
  if (box.lastChild && box.lastChild.classList.contains('sep')) box.lastChild.remove();
  document.body.append(box);
  // Below the button (a submenu: beside its row), kept inside the window.
  const r = anchor.getBoundingClientRect(), w = box.offsetWidth, ht = box.offsetHeight;
  let x = parent ? r.right + 2 : r.right - w, y = parent ? r.top - 6 : r.bottom + 4;
  if (parent && x + w > innerWidth - 8) x = r.left - w - 2;
  if (!parent && y + ht > innerHeight - 8) y = Math.max(8, r.top - ht - 4);
  if (parent && y + ht > innerHeight - 8) y = Math.max(8, innerHeight - ht - 8);
  box.style.left = Math.max(8, Math.min(x, innerWidth - w - 8)) + 'px';
  box.style.top = y + 'px';
  if (!parent) { const first = box.querySelector('.mi:not([disabled])'); if (first) first.focus({ preventScroll: true }); }
  return box;
}
// Arrow keys move through a menu.
document.addEventListener('keydown', e => {
  const m = document.activeElement && document.activeElement.closest('.menu');
  if (!m || !['ArrowDown', 'ArrowUp'].includes(e.key)) return;
  e.preventDefault();
  const rows = [...m.querySelectorAll('.mi:not([disabled])')], i = rows.indexOf(document.activeElement);
  rows[(i + (e.key === 'ArrowDown' ? 1 : rows.length - 1)) % rows.length].focus();
});

// changed() runs after something on the item changed (watched, details …).
function itemMenu(it, anchor, changed = () => {}) {
  const admin = S.me.admin;
  const watched = it.leafCount ? it.viewedLeafCount === it.leafCount : it.viewCount > 0;
  const matchable = it.type === 'movie' || it.type === 'show';
  const play = isPlayable(it);
  // Extras (trailers …) are left out: they belong to their movie.
  const deletable = S.me.can_delete && it.type !== 'clip' && !(it.extra || {}).extra_type;
  openMenu(anchor, [
    { label: t('Watch together'), run: () => watchTogether(it) },
    { label: t('Share'), run: () => shareItem(it) },
    { label: t('Add to queue'), run: () => addToQueue(it) },
    it.type === 'clip' ? null : { label: it.watchlist ? t('Remove from watchlist') : t('Add to watchlist'), run: async () => { await toggleWatchlist(it); changed(); } },
    { label: t('Add to …'), sub: () => addToEntries(it, changed) },
    { label: watched ? t('Mark as unwatched') : t('Mark as watched'), run: async () => {
      await api('POST', `/:/${watched ? 'unscrobble' : 'scrobble'}?key=${it.id}`);
      if (it.leafCount) it.viewedLeafCount = watched ? 0 : it.leafCount;
      it.viewCount = watched ? 0 : Math.max(1, it.viewCount || 0); it.viewOffset = 0;
      toast(watched ? t('Marked as unwatched') : t('Marked as watched')); changed();
    } },
    '-',
    admin ? { label: t('Refresh metadata'), run: async () => {
      toast(t('Downloading details…'));
      const r = await api('POST', `/library/metadata/${it.id}/refresh`); toast(t(r.message), 4000); changed();
    } } : null,
    admin ? { label: t('Analyze'), run: async () => { await api('POST', `/library/metadata/${it.id}/analyze`); toast(t('Analyzing — progress is shown at the top')); pollActivity(); } } : null,
    admin && matchable ? { label: t('Fix match…'), run: async () => fixMatch(await fullItem(it)) } : null,
    admin ? { label: t('Optimize…'), run: () => optimizeDialog(it) } : null,
    S.me.can_download && it.type !== 'clip' ? { label: t('Download…'), run: () => downloadDialog([it]) } : null,
    admin && it.librarySectionID ? { label: t('Grant access…'), run: () => grantAccess(it) } : null,
    '-',
    { label: t('View play history'), run: () => historyDialog(it) },
    { label: t('Report a problem'), run: () => reportDialog(it) },
    play ? { label: t('Media info'), run: () => mediaInfo(it) } : null,
    deletable ? '-' : null,
    deletable ? { label: t('Delete…'), danger: true, run: () => deleteItem(it) } : null,
  ]);
}

// ---------- deleting files from disk (for whoever may: the server setting and the person's right)
async function deleteItem(it) {
  const name = it.type === 'episode' && it.grandparentTitle ? `${it.grandparentTitle} · ${it.title}` : it.type === 'season' ? `${it.parentTitle || ''} · ${seasonTitle(it.title)}` : it.title;
  const n = isPlayable(it) ? 1 : (it.leafCount || (await leavesOf(it)).length);
  const msg = isPlayable(it) ? t('Delete “{title}” from disk? The file is deleted for good; this cannot be undone.', { title: name })
    : t('Delete all {n} episodes of “{title}” from disk? The files are deleted for good; this cannot be undone.', { n, title: name });
  if (!(await sure(msg, { ok: t('Delete'), danger: true }))) return;
  const r = await api('DELETE', '/library/metadata/' + it.id);
  toast(t('{n} file(s) deleted', { n: r.deleted }), 4000);
  // Its own page (or that of a season or show it took along) no longer exists.
  const here = location.hash.match(/^#\/item\/(\d+)/);
  if (here && (Number(here[1]) === it.id || !(await GET('/library/metadata/' + here[1]).catch(() => null)))) location.hash = '#/section/' + it.librarySectionID;
  else render();
  reloadSections(false);
}

// "Add to …": playlists (and for the owner the collections of a movie or show).
async function addToEntries(it, changed) {
  const pls = meta(await GET('/playlists?kind=manual&owner=me&sort=title'));
  const add = async (id, title) => { const r = await api('POST', `/playlists/${id}/items`, { ids: [it.id] }); toast(r.added ? t('Added to {name}', { name: title }) : t('Already in that playlist')); };
  const out = [...pls.map(p => ({ label: p.title, run: () => add(p.id, p.title) })),
    pls.length ? '-' : null,
    { label: t('New playlist…'), run: async () => { const name = await ask(t('Name of the new playlist')); if (name) { await api('POST', '/playlists', { title: name, kind: 'manual', ids: [it.id] }); toast(t('Added to {name}', { name })); } } }];
  if (S.me.admin && (it.type === 'movie' || it.type === 'show')) {
    const full = await fullItem(it);
    const mine = ((full.extra || {}).collections || []);
    const all = (await GET(`/library/sections/${it.librarySectionID}/collections`)).MediaContainer.Metadata.map(c => c.title).filter(c => !mine.includes(c));
    const setCols = async (cols, msg) => { await api('PUT', '/library/metadata/' + it.id, { collections: cols }); toast(msg); changed(); };
    out.push('-', ...all.map(c => ({ label: t('Collection: {name}', { name: c }), run: () => setCols([...mine, c], t('Added to {name}', { name: c })) })),
      { label: t('New collection…'), run: async () => { const name = (await ask(t('Name of the new collection')) || '').trim(); if (name) await setCols([...mine, name], t('Added to {name}', { name })); } });
  }
  return out;
}

// A link to the item's page, for someone with an account here.
async function shareItem(it) {
  const url = location.origin + location.pathname + '#/item/' + it.id;
  const name = it.type === 'episode' && it.grandparentTitle ? `${it.grandparentTitle} · ${it.title}` : it.title;
  if (navigator.share && matchMedia('(pointer:coarse)').matches) { try { await navigator.share({ title: name, url }); return; } catch (e) { if (e.name === 'AbortError') return; } }
  copyBox(t('Share “{title}”', { title: name }), url, t('Anyone with an account on this server who may see this library can open it.'));
}
// Shows a link to copy (the clipboard needs https, so the text is selected as well).
function copyBox(title, url, note) {
  const inp = h('input', { value: url, readonly: true, style: 'width:100%', onclick: e => e.target.select() });
  const copy = async () => {
    inp.select();
    let done = false;
    try { if (navigator.clipboard && isSecureContext) { await navigator.clipboard.writeText(url); done = true; } } catch (e) { /* not allowed */ }
    if (!done) { try { done = document.execCommand('copy'); } catch (e) { /* old browser */ } }
    toast(done ? t('Link copied') : t('Press Ctrl+C to copy the link'));
  };
  const box = modal(title, [note ? h('p', { class: 'dim' }, note) : null, inp,
    h('div', { class: 'actions' }, h('button', { class: 'primary', onclick: copy }, t('Copy link')), h('button', { onclick: () => box.remove() }, t('Close')))]);
  inp.select();
  return box;
}

// ---------- queue: plays after what is playing now (kept per browser)
const Q = { list: [] };
try { Q.list = JSON.parse(localStorage.getItem('queue') || '[]'); } catch (e) { Q.list = []; }
function saveQueue() { try { localStorage.setItem('queue', JSON.stringify(Q.list)); } catch (e) { /* private window */ } drawQueueChip(); }
async function addToQueue(it) {
  const list = await leavesOf(it);
  if (!list.length) return toast(t('Nothing to add'));
  Q.list.push(...list.map(x => ({ id: x.id, type: x.type, title: x.title, grandparentTitle: x.grandparentTitle, parentIndex: x.parentIndex, index: x.index, thumb: x.thumb, duration: x.duration })));
  saveQueue();
  toast(list.length === 1 ? t('Added to the queue') : t('{n} episodes added to the queue', { n: list.length }));
}
const queueTitle = x => x.type === 'episode' ? `${x.grandparentTitle || ''} · ${t('S{n}', { n: x.parentIndex || 0 })}${t('E{n}', { n: x.index || 0 })} ${x.title}` : x.title;
function drawQueueChip() {
  let chip = $('#qchip');
  if (!chip) { chip = h('button', { id: 'qchip', class: 'small', onclick: queueDialog }); $('#top').insertBefore(chip, $('#who')); }
  chip.textContent = t('Queue ({n})', { n: Q.list.length });
  chip.style.display = Q.list.length && S.me ? '' : 'none';
}
function queueDialog() {
  const body = h('div', { class: 'list', style: 'max-height:55vh;overflow:auto' });
  const draw = () => {
    if (!Q.list.length) { box.remove(); return; }
    body.replaceChildren(...Q.list.map((x, i) => h('div', { class: 'li' },
      h('img', { src: withTok(x.thumb), loading: 'lazy', alt: '' }),
      h('div', { class: 'grow' }, h('b', {}, queueTitle(x)), h('div', { class: 'dim' }, fmtDur(x.duration))),
      i ? h('button', { class: 'small', title: t('Move up'), onclick: () => { Q.list.splice(i - 1, 0, Q.list.splice(i, 1)[0]); saveQueue(); draw(); } }, '↑') : null,
      h('button', { class: 'small', title: t('Remove'), onclick: () => { Q.list.splice(i, 1); saveQueue(); draw(); } }, '✕'))));
  };
  const box = modal(t('Queue'), [h('p', { class: 'dim' }, t('Plays one after the other, after whatever is playing now.')), body,
    h('div', { class: 'actions' },
      h('button', { class: 'primary', onclick: () => { box.remove(); if (P.it) return toast(t('The queue plays after this video')); const first = Q.list.shift(); saveQueue(); play(first, 0); } }, t('▶ Play the queue')),
      h('button', { class: 'danger', onclick: () => { Q.list = []; saveQueue(); box.remove(); } }, t('Clear the queue')),
      h('button', { onclick: () => box.remove() }, t('Close')))]);
  draw();
}

// ---------- optimize: a version every browser plays as it is
async function optimizeDialog(it) {
  const full = await fullItem(it);
  const has = (full.extra || {}).optimized;
  const kinds = [['tv', t('Optimized for TV (1080p, 8 Mbps)')], ['mobile', t('Optimized for mobile (720p, 2 Mbps)')], ['original', t('Original quality (same size, up to 20 Mbps)')]];
  const sel = h('select', {}, kinds.map(([v, l]) => h('option', { value: v }, l)));
  const box = modal(t('Optimize “{title}”', { title: it.title }), [
    h('p', { class: 'dim' }, t('Saves a copy as MP4 (H.264 video, stereo AAC sound) that browsers, phones and TVs play directly. It is played instead of the original whenever the original would have to be converted, which saves the server work while watching. Copies are kept in the server’s data folder (optimized).')),
    !isPlayable(it) ? h('p', { class: 'dim' }, t('Every episode is optimized, one after the other.')) : null,
    has ? h('p', {}, t('Already optimized: {kind}, {size}.', { kind: (kinds.find(k => k[0] === has.quality) || [0, has.quality])[1], size: fmtSize(has.size || 0) })) : null,
    formRow(t('Quality'), sel),
    h('div', { class: 'actions' },
      h('button', { class: 'primary', onclick: async () => {
        try { const r = await api('POST', `/library/metadata/${it.id}/optimize`, { quality: sel.value }); toast(t('Optimizing {n} video(s) — progress is shown at the top', { n: r.videos }), 5000); box.remove(); pollActivity(); } catch (e) { toast(e.message, 6000); }
      } }, t('Optimize')),
      has || !isPlayable(it) ? h('button', { class: 'danger', onclick: async () => { const r = await api('DELETE', `/library/metadata/${it.id}/optimize`); toast(t('{n} optimized version(s) deleted', { n: r.removed })); box.remove(); } }, t('Delete optimized versions')) : null,
      h('button', { onclick: () => box.remove() }, t('Cancel')))]);
}

// ---------- access: who may see the library this is in (or a library itself)
const grantAccess = it => grantSectionAccess(Number(it.librarySectionID), true);
async function grantSectionAccess(sid, forItem) {
  const sec = S.sections.find(s => s.id === sid) || { title: '#' + sid };
  const users = (await GET('/api/users')).users.filter(u => !u.admin);
  const may = u => u.allowed_sections === null || u.allowed_sections.includes(sid);
  const checks = users.map(u => ({ u, el: h('input', { type: 'checkbox', checked: may(u) }) }));
  const box = modal(t('Grant access'), [
    h('p', { class: 'dim' }, forItem ? t('Access is given per library: ticked people may see everything in “{name}”, including this title.', { name: sec.title })
      : t('Ticked people may see everything in “{name}”.', { name: sec.title })),
    users.length ? h('div', { class: 'checks' }, checks.map(c => h('label', {}, c.el, c.u.username))) : h('p', {}, t('No other users yet. Invite someone under Users & invites.')),
    h('div', { class: 'actions' },
      users.length ? h('button', { class: 'primary', onclick: async () => {
        for (const { u, el } of checks) {
          if (el.checked === may(u)) continue;
          const now = u.allowed_sections === null ? S.sections.map(s => s.id) : u.allowed_sections;
          await api('PATCH', '/api/users/' + u.id, { allowed_sections: el.checked ? [...new Set([...now, sid])] : now.filter(x => x !== sid) });
        }
        toast(t('Saved')); box.remove();
      } }, t('Save')) : h('button', { class: 'primary', onclick: () => { box.remove(); location.hash = '#/users'; } }, t('Users & invites')),
      h('button', { onclick: () => box.remove() }, t('Cancel')))]);
}

// ---------- play history
async function historyDialog(it) {
  const r = await GET(`/library/metadata/${it.id}/history`);
  const what = x => x.type === 'episode' ? `${t('S{n}', { n: x.parentIndex || 0 })}${t('E{n}', { n: x.index || 0 })} · ${x.title}` : x.title;
  const box = modal(t('Play history: {title}', { title: it.title }), [
    r.history.length ? h('table', {}, h('tr', {}, h('th', {}, t('When')), S.me.admin ? h('th', {}, t('Who')) : null, isPlayable(it) ? null : h('th', {}, t('Episode')), h('th', {}, '')),
      r.history.map(e => h('tr', {}, h('td', {}, fmtDateTime(e.at)), S.me.admin ? h('td', {}, e.user || '–') : null,
        isPlayable(it) ? null : h('td', {}, h('a', { href: '#/item/' + e.item.id, onclick: () => box.remove() }, what(e.item))),
        h('td', { class: 'dim' }, e.manual ? t('marked as watched') : t('watched')))))
      : h('p', { class: 'dim' }, t('Not watched yet.')),
    h('div', { class: 'actions' }, h('button', { onclick: () => box.remove() }, t('Close')))]);
}

// ---------- report a problem
const PROBLEMS = [['playback', 'Does not play / stops'], ['video', 'Picture problem'], ['audio', 'Sound problem'], ['subtitles', 'Subtitle problem'], ['metadata', 'Wrong details or poster'], ['other', 'Something else']];
function reportDialog(it) {
  const kind = h('select', {}, PROBLEMS.map(([v, l]) => h('option', { value: v }, t(l))));
  const note = h('textarea', { placeholder: t('What happens? (optional)'), style: 'font-family:inherit;font-size:14px;min-height:90px' });
  const box = modal(t('Report a problem'), [
    h('p', { class: 'dim' }, t('The owner of this server sees your report.')),
    formRow(t('Problem'), kind), formRow(t('Details'), note, null, true),
    h('div', { class: 'actions' },
      h('button', { class: 'primary', onclick: async () => { await api('POST', `/library/metadata/${it.id}/report`, { kind: kind.value, note: note.value }); toast(t('Thanks — the problem was reported')); box.remove(); } }, t('Send')),
      h('button', { onclick: () => box.remove() }, t('Cancel')))]);
}
async function reportsPage() {
  markNav('reports');
  const r = await GET('/api/reports');
  const kindName = k => t((PROBLEMS.find(p => p[0] === k) || [k, k])[1]);
  const what = x => x.type === 'episode' ? `${x.grandparentTitle || ''} · ${t('S{n}', { n: x.parentIndex || 0 })}${t('E{n}', { n: x.index || 0 })} ${x.title}` : (x.title || t('(removed)'));
  set(h('h1', {}, t('Reported problems')),
    r.reports.length ? h('table', {}, h('tr', {}, h('th', {}, t('What')), h('th', {}, t('Problem')), h('th', {}, t('Who')), h('th', {}, t('When')), h('th', {}, '')),
      r.reports.map(p => h('tr', { style: p.resolved ? 'opacity:.55' : '' },
        h('td', {}, p.item.title ? h('a', { href: '#/item/' + p.item.id }, h('b', {}, what(p.item))) : what(p.item)),
        h('td', {}, kindName(p.kind), p.note ? h('div', { class: 'dim', style: 'white-space:pre-wrap' }, p.note) : null),
        h('td', {}, p.user || '–'), h('td', {}, fmtDateTime(p.at)),
        h('td', { style: 'white-space:nowrap' },
          h('button', { class: 'small', onclick: async () => { await api('PUT', '/api/reports/' + p.id, { resolved: !p.resolved }); reportsPage(); } }, p.resolved ? t('Reopen') : t('Done')), ' ',
          h('button', { class: 'small danger', onclick: async () => { await api('DELETE', '/api/reports/' + p.id); reportsPage(); } }, t('Delete'))))))
      : h('div', { class: 'empty' }, t('No problems reported.')));
}

// ---------- media info: every stream of the file
async function mediaInfo(it) {
  const full = await fullItem(it);
  const m = (full.Media || [])[0];
  if (!m) return toast(t('This entry has no video file'));
  const kv = rows => h('div', { class: 'kv' }, rows.filter(r => r && r[1] !== undefined && r[1] !== null && r[1] !== '' && r[1] !== 0).map(([k, v]) => [h('div', { class: 'dim' }, k), h('div', { style: 'word-break:break-all' }, String(v))]));
  const kinds = { video: t('Video'), audio: t('Audio'), subtitle: t('Subtitles') };
  const streams = (m.streams || []).map(s => [h('h3', { style: 'margin:14px 0 4px' }, `${kinds[s.kind] || s.kind} #${s.index}${s.default ? ' · ' + t('default') : ''}`), kv([
    [t('Codec'), s.codec + (s.profile ? ` (${s.profile})` : '')], [t('Language'), s.language], [t('Title'), s.title],
    s.width ? [t('Resolution'), `${s.width}×${s.height}`] : null, [t('Bit depth'), s.bit_depth ? t('{n}-bit', { n: s.bit_depth }) : ''],
    [t('HDR'), s.dovi ? 'Dolby Vision' : s.hdr10plus ? 'HDR10+' : s.color_transfer === 'smpte2084' ? 'HDR10' : s.color_transfer === 'arib-std-b67' ? 'HLG' : ''],
    [t('Channels'), s.channels ? `${s.channels}${s.channel_layout ? ' (' + s.channel_layout + ')' : ''}` : '']])]);
  const opt = (full.extra || {}).optimized;
  const box = modal(t('Media info'), [
    kv([[t('File'), m.file], [t('File type'), `${(m.container || '').toUpperCase()}${m.formatName ? ' (' + m.formatName + ')' : ''}`], [t('Size'), m.size ? fmtSize(m.size) : ''],
      [t('Length'), fmtDur(full.duration)], [t('Bitrate'), m.bitrate ? fmtNum(m.bitrate) + ' kbit/s' : ''],
      (m.Part || []).length > 1 ? [t('Parts'), t('{n} files, played one after the other', { n: m.Part.length })] : null,
      opt ? [t('Optimized version'), `${opt.quality}, ${fmtSize(opt.size || 0)}`] : null,
      [t('Subtitle files'), (m.subtitles || []).filter(s => !s.embedded).map(s => s.file).join(', ')]]),
    m.probed === false ? h('p', { class: 'dim' }, t('The file has not been read with ffprobe yet — use Analyze.')) : null,
    streams,
    h('div', { class: 'actions' }, h('button', { onclick: () => box.remove() }, t('Close')))]);
}

// ---------- watch together: everyone in a room plays the same thing at the same place
const TG = { code: null, seq: 0, timer: null, quiet: 0, members: [] };
async function watchTogether(it) {
  const list = await leavesOf(it);
  const target = list.find(x => !x.viewCount) || list[0];
  if (!target) return toast(t('Nothing to play'));
  const r = await api('POST', '/api/together', { item: target.id, position: target.viewOffset || 0 });
  const url = location.origin + location.pathname + '#/together/' + r.code;
  const box = copyBox(t('Watch together'), url, t('Send this link to the people you want to watch with (they need an account here that may see this library). Whoever plays, pauses or jumps moves everyone.'));
  box.querySelector('.actions').prepend(h('button', { class: 'primary', onclick: () => { box.remove(); joinTogether(r.code); } }, t('▶ Start watching')));
}
async function joinTogether(code) {
  let st;
  try { st = await GET('/api/together/' + code); } catch (e) { return set(h('div', { class: 'empty' }, e.message)); }
  history.replaceState(null, '', location.pathname + location.search + '#/item/' + st.item);
  TG.code = code; TG.seq = st.seq; TG.quiet = Date.now() + 3000;
  await play({ id: st.item }, st.position / 1000, [], true);
  if (st.paused) video.pause();
  tgDraw(st.members);
  clearInterval(TG.timer);
  TG.timer = setInterval(tgPoll, 2000);
  const others = st.members.filter(m => m.id !== S.me.id).map(m => m.name);
  toast(others.length ? t('Joined — watching with {names}', { names: others.join(', ') }) : t('Waiting for the others — playing starts for everyone when you press play'), 5000);
}
function tgDraw(members) {
  TG.members = members || TG.members;
  const others = TG.members.filter(m => m.id !== S.me.id).map(m => m.name);
  $('#ptogether').textContent = TG.code ? ' · ' + (others.length ? t('together with {names}', { names: others.join(', ') }) : t('together (waiting for others)')) : '';
}
async function tgPoll() {
  if (!TG.code || !P.it) return tgLeave();
  let st;
  try { st = await GET('/api/together/' + TG.code); } catch (e) { toast(e.message, 5000); return tgLeave(); }
  const before = TG.members.map(m => m.name).join();
  tgDraw(st.members);
  const now = TG.members.map(m => m.name).join();
  if (before && before !== now) toast(t('Watching together: {names}', { names: TG.members.map(m => m.name).join(', ') }));
  if (st.seq <= TG.seq) return;
  TG.seq = st.seq;
  TG.quiet = Date.now() + 2000;
  if (String(st.item) !== String(P.it.id)) { await play({ id: st.item }, st.position / 1000, [], true); if (st.paused) video.pause(); return; }
  const want = st.position / 1000;
  if (Math.abs(position() - want) > 2) await seekTo(want);
  if (st.paused && !video.paused) video.pause();
  if (!st.paused && video.paused) video.play().catch(() => toast(t('{who} pressed play — click the video to play along', { who: st.by }), 5000));
}
// Something changed here: tell the others (not while following a change of theirs).
function tgSend(extra) {
  if (!TG.code || !P.it || Date.now() < TG.quiet) return;
  api('POST', '/api/together/' + TG.code, { position: Math.round(position() * 1000), paused: video.paused, ...extra })
    .then(st => { TG.seq = Math.max(TG.seq, st.seq); }).catch(() => {});
}
function tgLeave() {
  clearInterval(TG.timer);
  if (TG.code) api('DELETE', '/api/together/' + TG.code).catch(() => {});
  TG.code = null; TG.members = [];
  const el = $('#ptogether'); if (el) el.textContent = '';
}
video.addEventListener('play', () => tgSend());
video.addEventListener('pause', () => { if (!video.ended) tgSend(); });
video.addEventListener('seeked', () => { if (P.method === 'direct' && !split()) tgSend(); });

// ---------- the left menu: libraries and Playlists in the order each person chose
// (kept on the server per person), unpinned ones under "More"; "⋯" edits it.
const NAV = { edit: false, cur: '', more: false };
try { NAV.more = localStorage.getItem('navMore') === '1'; } catch (e) { /* private window */ }
function navList() {
  const all = [...S.sections.map(s => ({ key: 's' + s.id, sec: s, title: s.title })), { key: 'playlists', title: t('Playlists') }];
  const sb = (S.me && S.me.sidebar) || {};
  const order = sb.order || [], hidden = new Set(sb.hidden || []);
  // New libraries come after the ones already placed, in the server's order.
  const pos = new Map(all.map((e, i) => [e.key, order.includes(e.key) ? order.indexOf(e.key) : order.length + i]));
  return all.sort((a, b) => pos.get(a.key) - pos.get(b.key)).map(e => Object.assign(e, { hidden: hidden.has(e.key) }));
}
async function putNav(sb) {
  S.me.sidebar = sb; drawNav();
  try { await api('PUT', '/api/me/sidebar', sb); } catch (e) { toast(e.message, 5000); }
}
const saveNav = list => putNav({ order: list.map(e => e.key), hidden: list.filter(e => e.hidden).map(e => e.key) });
function setPinned(key, pinned) {
  const list = navList();
  const e = list.find(x => x.key === key); if (!e) return;
  list.splice(list.indexOf(e), 1);
  e.hidden = !pinned;
  // Pinned again: after the last pinned entry. Unpinned: at the end (under "More").
  if (pinned) { let at = 0; list.forEach((x, i) => { if (!x.hidden) at = i + 1; }); list.splice(at, 0, e); } else list.push(e);
  saveNav(list);
  toast(pinned ? t('Pinned to the menu') : t('Unpinned — it is under “More” in the menu'));
}
const moreBtn = (title, onclick) => h('button', { class: 'more', title, 'aria-label': title, onclick: e => { e.preventDefault(); e.stopPropagation(); onclick(e.currentTarget); } }, '⋯');
function navLink(e) {
  const s = e.sec;
  return h('a', { href: s ? '#/section/' + s.id : '#/playlists', 'data-nav': s ? 'section/' + s.id : 'playlists' },
    h('span', {}, e.title), s ? h('small', {}, s.type === 'show' ? t('{n} shows', { n: s.count }) : s.videoCount) : null,
    moreBtn(t('More'), b => s ? sectionMenu(s, b) : openMenu(b, [{ label: e.hidden ? t('Pin') : t('Unpin'), run: () => setPinned(e.key, e.hidden) }])));
}
function drawNav() {
  const box = $('#navsections');
  if (!S.me) return box.replaceChildren();
  const list = navList();
  const sb = S.me.sidebar || {};
  const head = h('div', { class: 'head navhead' }, h('span', {}, t('LIBRARIES')),
    NAV.edit ? h('button', { class: 'small primary', onclick: () => { NAV.edit = false; drawNav(); } }, t('Done'))
      : moreBtn(t('Edit the menu'), b => openMenu(b, [
        { label: t('Edit the menu'), run: () => { NAV.edit = true; drawNav(); } },
        list.some(e => e.hidden) ? { label: t('Pin all'), run: () => putNav({ order: sb.order || [], hidden: [] }) } : null,
        (sb.order || []).length || (sb.hidden || []).length ? { label: t('Back to the standard order'), run: () => putNav({ order: [], hidden: [] }) } : null,
        S.me.admin ? '-' : null,
        S.me.admin ? { label: t('Add a library…'), run: () => { location.hash = '#/libraries'; } } : null])));
  const pinned = list.filter(e => !e.hidden), off = list.filter(e => e.hidden);
  if (NAV.edit) {
    box.replaceChildren(head, h('p', { class: 'dim navhint' }, t('Drag to sort, or use the arrows. ✕ unpins, + pins again.')),
      ...pinned.map(e => editRow(e, list)), off.length ? h('div', { class: 'edit-sep' }, t('NOT PINNED')) : '', ...off.map(e => editRow(e, list)));
  } else {
    // "More" opens by itself while one of its libraries is shown.
    const open = NAV.more || off.some(e => (e.sec ? 'section/' + e.sec.id : 'playlists') === NAV.cur);
    box.replaceChildren(head, ...pinned.map(navLink),
      off.length ? h('a', { href: '#', class: 'morehead', onclick: ev => {
        ev.preventDefault(); NAV.more = !open;
        try { localStorage.setItem('navMore', NAV.more ? '1' : '0'); } catch (x) { /* private window */ }
        drawNav();
      } }, h('span', {}, t('More')), h('small', {}, (open ? '▾ ' : '▸ ') + off.length)) : '',
      ...(open ? off.map(navLink) : []));
  }
  document.querySelectorAll('#side a').forEach(a => a.classList.toggle('on', a.dataset.nav === NAV.cur));
}
// One line of the menu while it is being edited: drag it, move it up or down, (un)pin it.
function editRow(e, list) {
  const group = list.filter(x => x.hidden === e.hidden), i = group.indexOf(e);
  const move = d => { const a = list.indexOf(e), b = list.indexOf(group[i + d]); [list[a], list[b]] = [list[b], list[a]]; saveNav(list); };
  const clear = () => row.classList.remove('over-top', 'over-bottom');
  const below = ev => ev.clientY > row.getBoundingClientRect().top + row.offsetHeight / 2;
  const row = h('div', { class: 'edit-row' + (e.hidden ? ' off' : ''), draggable: 'true',
    ondragstart: ev => { ev.dataTransfer.setData('text/plain', e.key); ev.dataTransfer.effectAllowed = 'move'; row.classList.add('drag'); },
    ondragend: () => row.classList.remove('drag'),
    ondragover: ev => { ev.preventDefault(); clear(); row.classList.add(below(ev) ? 'over-bottom' : 'over-top'); },
    ondragleave: clear,
    ondrop: ev => {
      ev.preventDefault(); clear();
      const from = list.find(x => x.key === ev.dataTransfer.getData('text/plain'));
      if (!from || from === e) return;
      list.splice(list.indexOf(from), 1);
      // Dropped among the pinned entries it is pinned, among the others unpinned.
      from.hidden = e.hidden;
      list.splice(list.indexOf(e) + (below(ev) ? 1 : 0), 0, from);
      saveNav(list);
    } },
    h('span', { class: 'grip', 'aria-hidden': 'true' }, '⠿'), h('span', { class: 'n', title: e.title }, e.title),
    h('button', { title: t('Move up'), 'aria-label': t('Move up'), disabled: i === 0, onclick: () => move(-1) }, '↑'),
    h('button', { title: t('Move down'), 'aria-label': t('Move down'), disabled: i === group.length - 1, onclick: () => move(1) }, '↓'),
    h('button', { title: e.hidden ? t('Pin') : t('Unpin'), 'aria-label': e.hidden ? t('Pin') : t('Unpin'), onclick: () => setPinned(e.key, e.hidden) }, e.hidden ? '+' : '✕'));
  return row;
}

// ---------- a library's "⋯" menu (in the left menu, on its page and under Libraries)
// Reads the libraries again; on the Libraries page or a library's page that page is drawn again too.
async function reloadSections(rerender = true) {
  try { S.sections = (await GET('/library/sections')).MediaContainer.Directory; } catch (e) { return; }
  drawChrome();
  if (rerender && /^#\/(libraries|section\/)/.test(location.hash)) render();
}
function sectionMenu(sec, anchor) {
  const key = 's' + sec.id;
  const e = navList().find(x => x.key === key);
  const hidden = !!(e && e.hidden);
  const admin = S.me.admin;
  openMenu(anchor, [
    admin ? { label: t('Scan library files'), run: async () => {
      await api('POST', `/library/sections/${sec.id}/refresh`); toast(t('Scanning {name}', { name: sec.title })); pollActivity();
      setTimeout(() => reloadSections(location.hash === '#/libraries'), 800);
    } } : null,
    admin ? { label: t('Grant access…'), run: () => grantSectionAccess(sec.id) } : null,
    admin ? { label: t('Manage library'), sub: async () => {
      const fresh = await GET('/library/sections/' + sec.id);
      return [
        { label: t('Edit…'), run: () => editSection(fresh) },
        { label: t('Refresh all metadata'), run: async () => {
          const r = await api('POST', `/library/sections/${sec.id}/refreshMetadata`);
          toast(t('Downloading details for {n} movies and shows again — progress is shown at the top', { n: r.queued }), 5000); pollActivity();
        } },
        { label: fresh.trash ? t('Empty trash ({n})', { n: fresh.trash }) : t('Empty trash'), run: () => emptyTrash(fresh) },
        { label: t('Analyze'), run: async () => {
          const r = await api('POST', `/library/sections/${sec.id}/analyze`);
          toast(t('Analyzing {n} videos — progress is shown at the top', { n: r.videos }), 5000); pollActivity();
        } },
        '-',
        { label: t('Delete library…'), danger: true, run: () => removeSection(fresh) },
      ];
    } } : null,
    admin ? '-' : null,
    { label: hidden ? t('Pin') : t('Unpin'), run: () => setPinned(key, hidden) },
  ]);
}
async function emptyTrash(sec) {
  if (!sec.trash) return toast(t('The trash of “{name}” is empty', { name: sec.title }));
  if (!(await sure(t('Take {n} unavailable items out of “{name}” for good? Their watched state and details go with them.', { n: sec.trash, name: sec.title }), { ok: t('Empty trash'), danger: true }))) return;
  const r = await api('POST', `/library/sections/${sec.id}/emptyTrash`);
  toast(t('{n} items taken out', { n: r.removed })); reloadSections();
}
async function removeSection(sec) {
  if (!(await sure(t('Remove the library "{name}"? Your files stay on disk.', { name: sec.title }), { ok: t('Remove'), danger: true }))) return;
  await api('DELETE', '/library/sections/' + sec.id);
  toast(t('Library removed'));
  if (location.hash.startsWith('#/section/' + sec.id)) { location.hash = '#/'; reloadSections(false); } else reloadSections();
}
// "Edit…": name, details language, credits detection and folders in one window.
async function editSection(sec) {
  const st = await GET('/api/settings');
  const serverLang = (st.languages.find(l => l.code === st.settings.metadata_language) || {}).name || st.settings.metadata_language;
  const name = h('input', { value: sec.title, style: 'width:100%' });
  const lang = h('select', {}, h('option', { value: '', selected: !sec.language }, t('Same as server ({lang})', { lang: serverLang })),
    st.languages.map(l => h('option', { value: l.code, selected: sec.language === l.code }, l.name)));
  const credits = h('input', { type: 'checkbox', checked: sec.creditsDetection !== false });
  const folders = folderRows(sec.paths);
  const box = modal(t('Edit library'), [
    formRow(t('Name'), name), formRow(t('Kind'), h('span', {}, sectionKind(sec.type))),
    formRow(t('Language of titles and plots'), lang, t('Changing it downloads this library’s details again.')),
    formRow(t('Enable credits detection'), credits),
    formRow(t('Folders'), folders.el, t('Removing a folder takes its videos out of the library; the files stay on disk.'), true),
    h('div', { class: 'actions', style: 'margin-top:14px' },
      h('button', { class: 'primary', onclick: async () => {
        const paths = folders.values();
        if (!paths.length) return toast(t('Give at least one folder in paths'));
        if (!name.value.trim()) return toast(t('Give the library a name'));
        const body = { name: name.value.trim(), language: lang.value, credits_detection: credits.checked };
        if (paths.join('\n') !== sec.paths.join('\n')) body.paths = paths;
        try {
          const r = await api('PUT', '/library/sections/' + sec.id, body);
          toast(r.refetching ? t('Downloading details for {n} movies and shows again — progress is shown at the top', { n: r.refetching }) : body.paths ? t('Saved; scanning') : t('Saved'), 5000);
          box.remove(); reloadSections(); pollActivity();
        } catch (e) { toast(e.message, 5000); }
      } }, t('Save')),
      h('button', { onclick: () => box.remove() }, t('Cancel')))]);
  name.focus();
}

// ---------- router
async function render() {
  const hash = location.hash.slice(1) || '/';
  const [path, qs] = hash.split('?');
  const parts = path.split('/').filter(Boolean);
  if (!S.status) { try { S.status = await GET('/api/status'); } catch (e) { return set(h('div', { class: 'empty' }, t('The server cannot be reached.'))); } }
  try {
    if (parts[0] === 'invite' && parts[1]) return await invitePage(parts[1]);
    if (S.status.setup_needed) return setupPage();
    if (!S.me) return loginPage();
    switch (parts[0]) {
      case undefined: return await homePage();
      case 'section': return await sectionPage(parts[1], qs);
      case 'item': return await itemPage(parts[1]);
      case 'person': return await personPage(parts[1], qs);
      case 'search': return await searchPage(decodeURIComponent(parts[1] || ''));
      case 'playlists': return await playlistsPage(qs);
      case 'playlist': return await playlistPage(parts[1], qs);
      case 'libraries': return await librariesPage();
      case 'users': return await usersPage();
      case 'plugins': return await pluginsPage();
      case 'settings': return await settingsPage();
      case 'api': return await apiPage();
      case 'reports': return await reportsPage();
      case 'watchlist': return await watchlistPage(qs);
      case 'downloads': return await downloadsPage();
      case 'dashboard': return await dashboardPage();
      case 'together': return await joinTogether(parts[1]);
      case 'login': location.hash = '#/'; return;
      default: return set(h('div', { class: 'empty' }, t('Page not found.')));
    }
  } catch (e) { set(h('div', { class: 'empty' }, t('Something went wrong: {msg}', { msg: e.message }))); }
}
window.addEventListener('hashchange', () => { if (!P.it) render(); });
boot();
