// Lichtspiel web app, part 2: playlists, smart playlist editor, admin pages, API page, router.
'use strict';

// ---------- playlists
async function playlistsPage(qs) {
  markNav('playlists');
  const p = new URLSearchParams(qs || '');
  const st = { kind: p.get('kind') || '', owner: p.get('owner') || '', sort: p.get('sort') || 'pinned:desc,title', title: p.get('title') || '' };
  const go = () => { const n = new URLSearchParams(); for (const [k, v] of Object.entries(st)) if (v) n.set(k, v); location.hash = '#/playlists?' + n; };
  const q = new URLSearchParams(); for (const [k, v] of Object.entries(st)) if (v) q.set(k, v);
  const list = meta(await GET('/playlists?' + q));
  const sel = (key, opts) => h('select', { onchange: e => { st[key] = e.target.value; go(); } }, opts.map(([v, l]) => h('option', { value: v, selected: st[key] === v }, l)));
  set(h('h1', {}, 'Playlists'),
    h('div', { class: 'bar-tools' },
      sel('kind', [['', 'All kinds'], ['default', 'Default'], ['smart', 'Smart'], ['manual', 'Normal']]),
      sel('owner', [['', 'Everyone\'s'], ['me', 'Mine'], ['others', 'Shared with me'], ['server', 'From the server']]),
      sel('sort', [['pinned:desc,title', 'Pinned first'], ['title', 'Name A–Z'], ['title:desc', 'Name Z–A'], ['updated_at:desc', 'Recently changed'], ['created_at:desc', 'Newest'], ['item_count:desc', 'Most items'], ['duration:desc', 'Longest']]),
      h('input', { placeholder: 'Find by name', value: st.title, onchange: e => { st.title = e.target.value.trim(); go(); } }),
      h('span', { class: 'grow' }),
      h('button', { onclick: newManual }, '+ New playlist'),
      h('button', { class: 'primary', onclick: () => smartEditor(null) }, '+ New smart playlist')),
    list.length ? h('div', { class: 'list' }, list.map(pl => h('div', { class: 'li' },
      h('img', pl.thumb ? { src: withTok(pl.thumb) } : {}),
      h('div', { class: 'grow' }, h('a', { href: '#/playlist/' + pl.id }, h('b', {}, pl.title)), ' ',
        h('span', { class: 'tag ' + pl.kind }, pl.kind === 'manual' ? 'normal' : pl.kind), pl.shared && pl.kind !== 'default' ? h('span', { class: 'tag' }, 'shared') : null, pl.pinned ? h('span', { class: 'tag' }, 'on home') : null,
        h('div', { class: 'dim' }, `${pl.leafCount} items · ${fmtDur(pl.duration) || '0 min'} · by ${pl.owner} · changed ${fmtDate(pl.updatedAt)}`)),
      pl.leafCount ? h('button', { class: 'primary small', onclick: () => playPlaylist(pl.id) }, '▶ Play') : null))) : h('div', { class: 'empty' }, 'No playlists match.'));
}

async function newManual() {
  const t = prompt('Name of the new playlist');
  if (!t) return;
  const r = await api('POST', '/playlists', { title: t, kind: 'manual' });
  location.hash = '#/playlist/' + meta(r)[0].id;
}

async function playPlaylist(id, shuffle) {
  const items = meta(await GET(`/playlists/${id}/items${shuffle ? '?sort=random' : ''}`));
  if (!items.length) return toast('This playlist is empty');
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
  set(h('div', { class: 'dim' }, h('a', { href: '#/playlists' }, 'Playlists'), ' ›'),
    h('h1', {}, pl.title, ' ', h('span', { class: 'tag ' + pl.kind }, manual ? 'normal' : pl.kind)),
    pl.summary ? h('p', { class: 'dim' }, pl.summary) : null,
    h('div', { class: 'actions' },
      h('button', { class: 'primary', onclick: () => playPlaylist(id) }, '▶ Play all'),
      h('button', { onclick: () => playPlaylist(id, true) }, 'Shuffle'),
      pl.smart && pl.editable ? h('button', { onclick: () => smartEditor(pl) }, 'Edit rules') : null,
      pl.editable ? h('button', { onclick: async () => { const t = prompt('New name', pl.title); if (t) { await api('PUT', '/playlists/' + id, { title: t }); reload(); } } }, 'Rename') : null,
      pl.editable ? h('button', { onclick: async () => { await api('PUT', '/playlists/' + id, { pinned: !pl.pinned }); reload(); } }, pl.pinned ? 'Remove from home' : 'Show on home') : null,
      pl.editable && !pl.default ? h('button', { onclick: async () => { await api('PUT', '/playlists/' + id, { shared: !pl.shared }); reload(); } }, pl.shared ? 'Stop sharing' : 'Share with everyone') : null,
      pl.smart ? h('button', { onclick: async () => { const r = await api('POST', `/playlists/${id}/copy`, {}); location.hash = '#/playlist/' + r.id; } }, 'Save as normal playlist') : null,
      pl.editable && !pl.default ? h('button', { class: 'danger', onclick: async () => { if (confirm('Delete this playlist?')) { await api('DELETE', '/playlists/' + id); location.hash = '#/playlists'; } } }, 'Delete') : null),
    h('div', { class: 'bar-tools' },
      sel('sort', [['', manual ? 'Playlist order' : 'Rule order'], ...SORTS, ['episode_order', 'Show, season, episode']]),
      sel('type', [['', 'Movies and episodes'], ['movie', 'Movies'], ['episode', 'Episodes']]),
      sel('watched', [['', 'Watched or not'], ['unwatched', 'Unwatched'], ['watched', 'Watched']]),
      h('input', { placeholder: 'Filter by title', value: st.q, onchange: e => { st.q = e.target.value.trim(); go(); } }),
      h('span', { class: 'grow' }), h('span', { class: 'dim' }, `${res.totalSize} of ${pl.leafCount} items · ${fmtDur(pl.duration)}`)),
    items.length ? h('div', { class: 'list' }, items.map((it, i) => h('div', { class: 'li' },
      h('img', { src: withTok(it.type === 'episode' && it.grandparentThumb ? it.grandparentThumb : it.thumb), loading: 'lazy' }),
      h('div', { class: 'grow' }, h('a', { href: '#/item/' + it.id }, h('b', {}, it.type === 'episode' ? `${it.grandparentTitle} · S${it.parentIndex}E${it.index} ${it.title}` : it.title)),
        h('div', { class: 'dim' }, [it.year, fmtDur(it.duration), it.viewCount ? '✓ watched' : '', it.viewOffset ? 'stopped at ' + fmtTime(it.viewOffset / 1000) : ''].filter(Boolean).join(' · '))),
      reorder ? [h('button', { class: 'small', title: 'Move up', onclick: () => move(i, -1) }, '↑'), h('button', { class: 'small', title: 'Move down', onclick: () => move(i, 1) }, '↓')] : null,
      manual && pl.editable ? h('button', { class: 'small danger', onclick: async () => { await api('DELETE', `/playlists/${id}/items/${it.id}`); reload(); } }, 'Remove') : null,
      h('button', { class: 'primary small', onclick: () => play(it, it.viewOffset ? it.viewOffset / 1000 : 0, items.slice(i + 1)) }, '▶')))) :
      h('div', { class: 'empty' }, manual ? 'Empty. Open a movie or episode and use "Add to playlist".' : 'Nothing matches these rules right now.'));
}

function smartEditor(pl) {
  const fields = ((S.filters && S.filters.fields) || []).filter(f => f.type !== 'sort');
  const ops = (S.filters && S.filters.operators) || [];
  const q = pl && pl.query ? JSON.parse(JSON.stringify(pl.query)) : { match_all: true, rules: [{ field: 'kind', op: 'is', value: 'movie' }], sort: 'title', limit: null, sections: [] };
  const rulesBox = h('div');
  const preview = h('div', { class: 'dim', style: 'margin-top:10px' });
  const OPS = { bool: ['is', 'is_not'], date: ['in_last_days', 'not_in_last_days', 'gt', 'lt'], number: ['is', 'is_not', 'gt', 'gte', 'lt', 'lte', 'between', 'in'], text: ['is', 'is_not', 'contains', 'not_contains', 'starts_with', 'ends_with', 'in'] };
  const opsFor = type => ops.filter(o => (OPS[type] || OPS.text).includes(o.op));
  let t;
  const upd = () => { clearTimeout(t); t = setTimeout(async () => {
    try {
      const r = (await api('POST', '/playlists/preview', { query: q })).MediaContainer;
      preview.textContent = `${r.totalSize} items match: ` + r.Metadata.slice(0, 6).map(m => m.type === 'episode' ? `${m.grandparentTitle} S${m.parentIndex}E${m.index}` : m.title).join(', ') + (r.totalSize > 6 ? ' …' : '');
    } catch (e) { preview.textContent = e.message; }
  }, 250); };
  const drawRules = () => {
    rulesBox.replaceChildren(...q.rules.map((r, i) => {
      const f = fields.find(x => x.field === r.field) || fields[0];
      const valEl = f.type === 'bool'
        ? h('select', { onchange: e => { r.value = e.target.value === 'true'; upd(); } }, h('option', { value: 'true', selected: r.value === true }, 'yes'), h('option', { value: 'false', selected: r.value === false }, 'no'))
        : h('input', { value: Array.isArray(r.value) ? r.value.join(',') : (r.value ?? ''), placeholder: r.op === 'between' ? 'low,high' : r.op.includes('days') ? 'days' : 'value',
          onchange: e => { const v = e.target.value; r.value = r.op === 'between' ? v.split(',').map(Number) : (f.type === 'number' || r.op.includes('days')) && v !== '' && !isNaN(v) ? Number(v) : v; upd(); } });
      return h('div', { class: 'rule' },
        h('select', { onchange: e => { r.field = e.target.value; const nf = fields.find(x => x.field === r.field); r.op = opsFor(nf.type)[0].op; r.value = nf.type === 'bool' ? true : ''; drawRules(); upd(); } }, fields.map(x => h('option', { value: x.field, selected: x.field === r.field }, x.label))),
        h('select', { onchange: e => { r.op = e.target.value; drawRules(); upd(); } }, opsFor(f.type).map(o => h('option', { value: o.op, selected: o.op === r.op }, o.label))),
        valEl,
        h('button', { class: 'small danger', onclick: () => { q.rules.splice(i, 1); drawRules(); upd(); } }, '✕'));
    }));
  };
  const title = h('input', { value: pl ? pl.title : '', placeholder: 'e.g. Unwatched sci-fi from the 90s', style: 'width:100%' });
  const shared = h('input', { type: 'checkbox', checked: pl ? pl.shared : false });
  const pinned = h('input', { type: 'checkbox', checked: pl ? pl.pinned : false });
  const box = modal(pl ? 'Edit smart playlist' : 'New smart playlist', [
    h('label', {}, 'Name'), title,
    h('label', {}, 'Include items that match'),
    h('select', { onchange: e => { q.match_all = e.target.value === 'all'; upd(); } }, h('option', { value: 'all', selected: q.match_all }, 'all of these rules'), h('option', { value: 'any', selected: !q.match_all }, 'any of these rules')),
    h('div', { style: 'margin-top:10px' }, rulesBox),
    h('button', { class: 'small', onclick: () => { q.rules.push({ field: 'genre', op: 'is', value: '' }); drawRules(); } }, '+ Add rule'),
    h('label', {}, 'Sort by'),
    h('select', { onchange: e => { q.sort = e.target.value; upd(); } }, [...SORTS, ['episode_order', 'Show, season, episode']].map(([v, l]) => h('option', { value: v, selected: q.sort === v }, l))),
    h('label', {}, 'Limit (empty = no limit)'),
    h('input', { type: 'number', min: 1, value: q.limit || '', onchange: e => { q.limit = e.target.value ? Number(e.target.value) : null; upd(); } }),
    h('label', {}, 'Libraries'),
    h('div', { class: 'checks' }, S.sections.map(s => h('label', {}, h('input', { type: 'checkbox', checked: (q.sections || []).includes(s.id), onchange: e => { q.sections = (q.sections || []).filter(x => x !== s.id); if (e.target.checked) q.sections.push(s.id); upd(); } }), s.title)), h('span', { class: 'dim' }, '(none ticked = all)')),
    pl && pl.default ? null : h('div', { class: 'checks', style: 'margin-top:10px' }, h('label', {}, shared, 'Share with everyone'), h('label', {}, pinned, 'Show on home')),
    preview,
    h('div', { class: 'actions' }, h('button', { class: 'primary', onclick: async () => {
      if (!title.value.trim()) return toast('Give it a name');
      try {
        if (pl) { await api('PUT', '/playlists/' + pl.id, { title: title.value, query: q, shared: shared.checked, pinned: pinned.checked }); box.remove(); playlistPage(pl.id); }
        else { const r = await api('POST', '/playlists', { title: title.value, kind: 'smart', query: q, shared: shared.checked, pinned: pinned.checked }); box.remove(); location.hash = '#/playlist/' + meta(r)[0].id; }
      } catch (e) { toast(e.message, 5000); }
    } }, 'Save'), h('button', { onclick: () => box.remove() }, 'Cancel'))]);
  drawRules(); upd();
}

// ---------- admin: libraries
async function librariesPage() {
  const secs = (await GET('/library/sections')).MediaContainer.Directory;
  S.sections = secs; drawChrome(); markNav('libraries');
  const name = h('input', { placeholder: 'Movies', style: 'width:100%' });
  const type = h('select', {}, h('option', { value: 'movie' }, 'Movies'), h('option', { value: 'show' }, 'TV shows'), h('option', { value: 'other' }, 'Other videos'));
  const paths = h('textarea', { placeholder: 'One folder per line, e.g. /media/Movies or /volume1/video/Movies', style: 'min-height:70px' });
  const st = await GET('/api/settings');
  const langs = st.languages;
  const serverLang = (langs.find(l => l.code === st.settings.metadata_language) || {}).name || st.settings.metadata_language;
  const langSelect = (cur, list, onchange) => h('select', { onchange }, h('option', { value: '', selected: !cur }, `Same as server (${serverLang})`),
    list.map(l => h('option', { value: l.code, selected: cur === l.code }, l.name)));
  const newLang = langSelect('', langs);
  set(h('h1', {}, 'Libraries'),
    h('p', { class: 'dim' }, 'Movies and TV shows are kept in separate libraries. Name TV files like "Show/Season 01/Show - S01E02 - Title.mkv" and movies like "Title (Year).mkv". Every video container is picked up: ', h('code', {}, 'mkv mp4 avi mov ts m2ts wmv webm mpg vob flv ogv 3gp …')),
    secs.length ? h('div', { class: 'list' }, secs.map(s => h('div', { class: 'li' },
      h('div', { class: 'grow' }, h('b', {}, s.title), ' ', h('span', { class: 'tag' }, s.type), s.scanning ? h('span', { class: 'tag smart' }, 'scanning…') : null,
        h('div', { class: 'dim' }, `${s.type === 'show' ? s.count + ' shows, ' : ''}${s.videoCount} videos · last scan ${s.scannedAt ? new Date(s.scannedAt * 1000).toLocaleString() : 'never'}`),
        h('div', { class: 'dim', style: 'font-size:13px' }, s.paths.join(' · ')),
        h('div', { style: 'display:flex;gap:18px;flex-wrap:wrap;align-items:center;margin-top:6px' },
          h('label', { style: 'display:flex;gap:6px;align-items:center;margin:0;color:var(--text)', title: 'Detect credits and allow credits skipping for items in this library when enabled in server settings.' },
            h('input', { type: 'checkbox', checked: s.creditsDetection !== false, onchange: async e => { try { await api('PUT', '/library/sections/' + s.id, { credits_detection: e.target.checked }); toast('Saved'); } catch (er) { toast(er.message); } } }), 'Enable credits detection'),
          h('label', { style: 'display:flex;gap:6px;align-items:center;margin:0;color:var(--text)', title: 'Language of titles, plots and episode names downloaded for this library' }, 'Details language',
            langSelect(s.language || '', langs, async e => {
              try { const r = await api('PUT', '/library/sections/' + s.id, { language: e.target.value }); toast(r.refetching ? `Downloading details for ${r.refetching} movies and shows again — progress is shown at the top` : 'Saved', 6000); }
              catch (er) { toast(er.message); }
            })))),
      h('button', { onclick: async () => { await api('POST', `/library/sections/${s.id}/refresh`); toast('Scanning ' + s.title); setTimeout(librariesPage, 1500); } }, 'Scan now'),
      h('button', { title: 'Look for end credits in every video of this library now (from the chapters in the files)', onclick: async () => {
        try { const r = await api('POST', `/library/sections/${s.id}/credits`); toast(`Checking ${r.videos} videos for credits — progress is shown at the top`, 5000); pollActivity(); } catch (er) { toast(er.message, 6000); }
      } }, 'Detect credits'),
      h('button', { onclick: async () => { const t = prompt('Folders (separate with ;)', s.paths.join(';')); if (t !== null) { try { await api('PUT', '/library/sections/' + s.id, { paths: t }); } catch (e) { toast(e.message); } librariesPage(); } } }, 'Folders'),
      h('button', { class: 'danger', onclick: async () => { if (confirm(`Remove the library "${s.title}"? Your files stay on disk.`)) { await api('DELETE', '/library/sections/' + s.id); librariesPage(); } } }, 'Remove')))) : null,
    h('div', { class: 'panel', style: 'margin-top:20px' }, h('h2', { style: 'margin-top:0' }, 'Add a library'),
      h('label', {}, 'Name'), name, h('label', {}, 'Kind'), type, h('label', {}, 'Language of titles and plots'), newLang, h('label', {}, 'Folders'), paths,
      h('div', { class: 'actions' }, h('button', { onclick: () => folderPicker(p => { paths.value = (paths.value.trim() ? paths.value.trim() + '\n' : '') + p; }) }, 'Browse folders…'),
        h('button', { class: 'primary', onclick: async () => {
          try { await api('POST', '/library/sections', { name: name.value, type: type.value, paths: paths.value, language: newLang.value }); toast('Library added; scanning'); setTimeout(librariesPage, 1200); }
          catch (e) { toast(e.message, 5000); }
        } }, 'Add library'))),
    await importPanel());
}

// ---------- admin: import from the old server
async function importPanel() {
  const st = await GET('/api/import/old-server');
  const where = h('input', { value: st.detected || '', placeholder: 'Data folder of the old server, or its library .db file', style: 'width:100%' });
  const out = h('div', {});
  const lastBox = h('div', {});
  const showLast = r => {
    if (!r) return lastBox.replaceChildren();
    const f = r.files || {}, w = r.watched || {}, p = r.playlists || {};
    lastBox.replaceChildren(h('div', { class: 'panel', style: 'background:var(--panel2)' },
      h('b', {}, 'Last import'), ' ', h('span', { class: 'dim' }, r.finished_at ? new Date(r.finished_at * 1000).toLocaleString() : ''),
      h('div', { class: 'kv', style: 'margin-top:8px' },
        h('div', {}, 'Files found here'), h('div', {}, `${f.found_here} of ${f.in_old_database}`),
        h('div', {}, 'Libraries added'), h('div', {}, (r.libraries_created || []).map(l => l.name).join(', ') || 'none (existing ones used)'),
        h('div', {}, 'Watched / resume points / ratings'), h('div', {}, `${w.watched} / ${w.resume_points} / ${w.ratings}`),
        h('div', {}, 'Playlists'), h('div', {}, [...(p.created || []), ...(p.updated || [])].join(', ') || 'none'),
        h('div', {}, 'Credits markers'), h('div', {}, String(r.credits_markers)),
        h('div', {}, 'Titles and plots'), h('div', {}, `${r.details} items`),
        (p.smart_not_imported || []).length ? [h('div', {}, 'Not imported'), h('div', {}, 'smart playlists (their rules work differently here): ' + p.smart_not_imported.join(', '))] : null,
        (w.accounts_skipped || []).length ? [h('div', {}, 'Accounts skipped'), h('div', {}, w.accounts_skipped.join(', '))] : null),
      f.not_found ? h('details', { style: 'margin-top:8px' }, h('summary', { class: 'dim' }, `${f.not_found} files were not found here — check the folder translation`),
        h('pre', { style: 'white-space:pre-wrap;font-size:12px' }, (f.not_found_examples || []).join('\n'))) : null));
  };
  showLast(st.last);
  const look = async () => {
    out.replaceChildren(h('p', { class: 'dim' }, 'Reading the old database (a copy; the old server is not changed)…'));
    let pv, users;
    try { [pv, users] = await Promise.all([GET('/api/import/old-server/preview?db=' + encodeURIComponent(where.value)), GET('/api/users')]); }
    catch (e) { return out.replaceChildren(h('p', { class: 'err' }, e.message)); }
    const maps = [];
    const libs = pv.libraries.map(l => h('div', { style: 'margin:10px 0' },
      h('b', {}, l.name), ' ', h('span', { class: 'tag' }, l.type), l.imported ? null : h('span', { class: 'dim' }, ' — not imported (only movies and TV shows)'),
      l.imported ? l.folders.map(f => {
        const to = h('input', { value: f.suggested || f.old, style: 'flex:1;min-width:220px' });
        maps.push({ from: f.old, to });
        return h('div', { style: 'display:flex;gap:8px;align-items:center;margin-top:6px;flex-wrap:wrap' },
          h('code', { style: 'flex:1;min-width:220px;word-break:break-all' }, f.old), '→', to,
          h('button', { class: 'small', onclick: () => folderPicker(p => { to.value = p; }) }, 'Browse…'));
      }) : null));
    const acc = {};
    const accounts = pv.accounts.map(a => {
      const sel = h('select', {}, h('option', { value: '0' }, 'skip'), users.users.map(u => h('option', { value: u.id, selected: a.owner ? u.id === S.me.id : a.suggested_user === u.id }, u.username)));
      acc[a.id] = sel;
      return h('tr', {}, h('td', {}, a.name, a.owner ? h('span', { class: 'badge' }, 'owner') : ''), h('td', { class: 'dim' }, `${a.watched} watched`), h('td', {}, '→ ', sel));
    });
    const opt = {};
    const box = (k, label) => h('label', {}, (opt[k] = h('input', { type: 'checkbox', checked: true })), label);
    const c = pv.counts;
    out.replaceChildren(
      h('p', {}, `Found: ${c.movies} movies, ${c.shows} shows, ${c.episodes} episodes, ${c.watched_states} watched, ${c.resume_points} resume points, ${c.ratings} ratings, ${c.playlists} playlists (+${c.smart_playlists} smart), ${c.credits_markers} credits markers.`),
      h('h3', {}, 'Folders'), h('p', { class: 'dim' }, 'Where each folder of the old server is on this server. In Docker your video share is usually under /media.'), libs,
      h('h3', {}, 'People'), h('p', { class: 'dim' }, 'Whose watched state, ratings and playlists go to which user here. Invite the others first to bring theirs over, then import again.'),
      h('table', {}, accounts),
      h('h3', {}, 'What to bring over'),
      h('div', { class: 'checks' }, box('libraries', 'Libraries (added and scanned)'), box('watched', 'Watched, resume points and ratings'), box('playlists', 'Playlists'), box('markers', 'Credits markers'), box('details', 'Titles, plots and genres (kept; online details add posters and cast)')),
      h('div', { class: 'actions' }, h('button', { class: 'primary', onclick: async e => {
        const body = { db: pv.db, path_map: maps.map(m => ({ from: m.from, to: m.to.value.trim() })), accounts: {} };
        for (const [id, s] of Object.entries(acc)) if (s.value !== '0') body.accounts[id] = Number(s.value);
        for (const [k, el] of Object.entries(opt)) body[k] = el.checked;
        try { await api('POST', '/api/import/old-server', body); e.target.disabled = true; toast('Import started — progress is shown at the top', 5000); pollActivity(); waitDone(); }
        catch (er) { toast(er.message, 6000); }
      } }, 'Import')),
      pv.warnings.length ? h('p', { class: 'dim', style: 'font-size:12px' }, 'Notes: ' + pv.warnings.join('; ')) : null);
  };
  const waitDone = async () => {
    for (let i = 0; i < 2400; i++) {
      await new Promise(r => setTimeout(r, 1500));
      const s = await GET('/api/import/old-server').catch(() => null);
      if (s && !s.running) { showLast(s.last); toast('Import finished'); S.sections = (await GET('/library/sections')).MediaContainer.Directory; drawChrome(); return; }
    }
  };
  if (st.running) waitDone();
  return h('div', { class: 'panel', style: 'margin-top:20px' },
    h('h2', { style: 'margin-top:0' }, 'Import from your old media server'),
    h('p', { class: 'dim' }, 'Brings over libraries, everyone’s watched state, resume points and ratings, playlists, credits markers and titles/plots from the server this one replaces. It reads a copy of its library database (',
      h('code', {}, 'com.plexapp.plugins.library.db'), '); the old server keeps working. You can run it again any time — nothing is counted twice.'),
    h('label', {}, 'Old server’s data'), where,
    h('p', { class: 'dim', style: 'font-size:13px' }, st.detected ? 'Found automatically.' : 'Not found automatically. On a Synology with Docker, add the old server’s data folder to docker-compose.yml (see the README) — it then appears under /oldserver.'),
    h('div', { class: 'actions' }, h('button', { onclick: look }, 'Look inside')),
    out, lastBox);
}

function folderPicker(done) {
  const list = h('div', { class: 'list', style: 'max-height:50vh;overflow:auto' });
  const here = h('div', { class: 'dim', style: 'margin-bottom:8px;word-break:break-all' });
  let cur = '';
  const box = modal('Pick a folder', [here, list, h('div', { class: 'actions' }, h('button', { class: 'primary', onclick: () => { if (cur) { done(cur); box.remove(); } } }, 'Use this folder'), h('button', { onclick: () => box.remove() }, 'Cancel'))]);
  const open = async p => {
    try {
      const r = await GET('/api/browse?path=' + encodeURIComponent(p));
      cur = r.path; here.textContent = (r.path || 'Drives') + (r.videos_here ? ` — ${r.videos_here} videos here` : '');
      list.replaceChildren(...(r.parent !== null && r.parent !== undefined ? [h('div', { class: 'li', style: 'cursor:pointer', onclick: () => open(r.parent) }, '⬆ Up')] : []),
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
  const email = h('input', { type: 'email', placeholder: 'friend@example.com', style: 'width:100%' });
  const note = h('input', { placeholder: 'A short message (optional)', style: 'width:100%' });
  const days = h('input', { type: 'number', min: 1, max: 365, value: 7, style: 'width:90px' });
  const asAdmin = h('input', { type: 'checkbox' });
  const allLibs = h('input', { type: 'checkbox', checked: true });
  const libChecks = S.sections.map(s => ({ s, el: h('input', { type: 'checkbox', checked: true }) }));
  const result = h('div');
  const copy = url => { if (navigator.clipboard) navigator.clipboard.writeText(url); toast('Link copied'); };
  set(h('h1', {}, 'Users & invites'),
    h('div', { class: 'panel' }, h('h2', { style: 'margin-top:0' }, 'Invite someone'),
      h('p', { class: 'dim' }, 'You get a link to send them (by email, chat …). They open it, pick a user name and password, and can watch the libraries you tick.'),
      h('label', {}, 'Their email'), email, h('label', {}, 'Message'), note,
      h('label', {}, 'Link works for (days)'), days,
      h('label', {}, 'Libraries they may watch'),
      h('div', { class: 'checks' }, h('label', {}, allLibs, h('b', {}, 'All, including future ones')), libChecks.map(c => h('label', {}, c.el, c.s.title))),
      h('div', { class: 'checks', style: 'margin-top:10px' }, h('label', {}, asAdmin, 'Make them an admin')),
      h('div', { class: 'actions' }, h('button', { class: 'primary', onclick: async () => {
        const body = { email: email.value, note: note.value, days: Number(days.value) || 7, admin: asAdmin.checked };
        if (!allLibs.checked) body.allowed_sections = libChecks.filter(c => c.el.checked).map(c => c.s.id);
        const r = await api('POST', '/api/invites', body);
        const mail = `mailto:${encodeURIComponent(r.email)}?subject=${encodeURIComponent('Invitation to ' + (S.status ? S.status.name : 'my media server'))}&body=${encodeURIComponent((r.note ? r.note + '\n\n' : '') + 'Join here: ' + r.url)}`;
        result.replaceChildren(h('div', { class: 'panel', style: 'background:var(--panel2)' }, h('div', { class: 'ok' }, 'Invite ready. Send this link:'),
          h('input', { value: r.url, style: 'width:100%;margin:8px 0', onclick: e => e.target.select() }),
          h('button', { onclick: () => copy(r.url) }, 'Copy link'), ' ', r.email ? h('a', { href: mail }, h('button', {}, 'Open in email')) : null));
      } }, 'Create invite link')), result),
    h('h2', {}, 'People'),
    h('table', {}, h('tr', {}, h('th', {}, 'User'), h('th', {}, 'Libraries'), h('th', {}, 'Admin'), h('th', {}, 'May convert'), h('th', {}, '')),
      users.users.map(u => h('tr', {},
        h('td', {}, h('b', {}, u.username), h('div', { class: 'dim' }, u.email || ''), h('div', { class: 'dim', style: 'font-size:12px' }, 'joined ' + fmtDate(u.created_at))),
        h('td', {}, u.admin ? 'all (admin)' : u.allowed_sections === null ? 'all' : u.allowed_sections.map(secName).join(', ') || 'none', ' ',
          u.admin ? null : h('button', { class: 'small', onclick: () => accessEditor(u) }, 'Change')),
        h('td', {}, h('input', { type: 'checkbox', checked: u.admin, disabled: u.id === S.me.id, onchange: async e => { try { await api('PATCH', '/api/users/' + u.id, { admin: e.target.checked }); } catch (x) { toast(x.message); } usersPage(); } })),
        h('td', {}, h('input', { type: 'checkbox', checked: u.allow_transcode, title: 'Allow converting video on the fly', onchange: async e => { await api('PATCH', '/api/users/' + u.id, { allow_transcode: e.target.checked }); toast('Saved'); } })),
        h('td', {}, u.id === S.me.id ? h('span', { class: 'dim' }, 'you') : [
          h('button', { class: 'small', onclick: async () => { const pw = prompt('New password for ' + u.username); if (pw) { try { await api('PATCH', '/api/users/' + u.id, { password: pw }); toast('Password changed'); } catch (x) { toast(x.message); } } } }, 'Password'), ' ',
          h('button', { class: 'small danger', onclick: async () => { if (confirm('Remove ' + u.username + '?')) { await api('DELETE', '/api/users/' + u.id); usersPage(); } } }, 'Remove')])))),
    h('h2', {}, 'Invites'),
    invites.invites.length ? h('table', {}, h('tr', {}, h('th', {}, 'For'), h('th', {}, 'Status'), h('th', {}, 'Libraries'), h('th', {}, 'Link'), h('th', {}, '')),
      invites.invites.map(i => h('tr', {},
        h('td', {}, i.email || h('span', { class: 'dim' }, 'anyone with the link'), i.note ? h('div', { class: 'dim' }, i.note) : null),
        h('td', {}, i.status === 'used' ? `used by ${i.used_by}` : i.status === 'open' ? `open until ${fmtDate(i.expires_at)}` : 'expired'),
        h('td', {}, i.allowed_sections ? i.allowed_sections.map(secName).join(', ') : 'all'),
        h('td', {}, i.status === 'open' ? h('button', { class: 'small', onclick: () => copy(i.url) }, 'Copy link') : ''),
        h('td', {}, h('button', { class: 'small danger', onclick: async () => { await api('DELETE', '/api/invites/' + i.code); usersPage(); } }, i.status === 'open' ? 'Cancel' : 'Delete'))))) : h('p', { class: 'dim' }, 'No invites yet.'));
}

function accessEditor(u) {
  const all = h('input', { type: 'checkbox', checked: u.allowed_sections === null });
  const checks = S.sections.map(s => ({ s, el: h('input', { type: 'checkbox', checked: u.allowed_sections === null || u.allowed_sections.includes(s.id) }) }));
  const box = modal('Libraries for ' + u.username, [h('div', { class: 'checks' }, h('label', {}, all, h('b', {}, 'All, including future ones')), checks.map(c => h('label', {}, c.el, c.s.title))),
    h('div', { class: 'actions' }, h('button', { class: 'primary', onclick: async () => {
      await api('PATCH', '/api/users/' + u.id, { allowed_sections: all.checked ? null : checks.filter(c => c.el.checked).map(c => c.s.id) }); box.remove(); usersPage();
    } }, 'Save'), h('button', { onclick: () => box.remove() }, 'Cancel'))]);
}

// ---------- admin: plugins
async function pluginsPage() {
  markNav('plugins');
  const [r, log] = await Promise.all([GET('/api/plugins'), GET('/api/plugins/log')]);
  set(h('h1', {}, 'Plugins'),
    h('p', { class: 'dim' }, 'Plugins add features: fill in movie details, send notifications, react to playback, add their own API actions. Put each plugin in its own folder inside ', h('code', {}, r.folder), ' with a ', h('code', {}, 'plugin.json'), ' file, then press Reload. Any language works (Python, shell, Node …).'),
    h('div', { class: 'actions' }, h('button', { onclick: async () => { const x = await api('POST', '/api/plugins/reload'); toast(`${x.count} plugin(s) found`); pluginsPage(); } }, 'Reload plugins')),
    r.errors.length ? h('div', { class: 'panel', style: 'color:var(--red)' }, r.errors.map(e => h('div', {}, e))) : null,
    r.plugins.length ? r.plugins.map(p => {
      const ta = h('textarea', {}, JSON.stringify(p.settings, null, 2));
      return h('div', { class: 'panel' },
        h('div', { style: 'display:flex;gap:12px;align-items:center' }, h('b', { style: 'font-size:17px' }, p.name), h('span', { class: 'dim' }, 'v' + (p.version || '?')), p.author ? h('span', { class: 'dim' }, 'by ' + p.author) : null, h('span', { style: 'flex:1' }),
          h('span', { class: p.enabled ? 'ok' : 'dim' }, p.enabled ? 'on' : 'off'),
          h('button', { class: p.enabled ? '' : 'primary', onclick: async () => { await api('POST', `/api/plugins/${p.id}/${p.enabled ? 'disable' : 'enable'}`); pluginsPage(); } }, p.enabled ? 'Switch off' : 'Switch on')),
        h('p', {}, p.description), h('div', {}, 'Listens to: ', p.hooks.map(x => h('span', { class: 'tag' }, x))),
        h('label', {}, 'Settings'), ta,
        h('div', { class: 'actions' }, h('button', { onclick: async () => { try { await api('PUT', `/api/plugins/${p.id}/settings`, JSON.parse(ta.value)); toast('Saved'); } catch (e) { toast(e.message); } } }, 'Save settings')));
    }) : h('div', { class: 'empty' }, 'No plugins installed yet.'),
    h('h2', {}, 'Recent plugin activity'),
    log.log.length ? h('table', {}, log.log.slice(0, 40).map(l => h('tr', {}, h('td', { class: 'dim' }, new Date(l.at * 1000).toLocaleTimeString()), h('td', {}, l.plugin), h('td', {}, l.event), h('td', { class: l.ok ? 'ok' : 'err' }, l.ok ? 'ok' : 'failed'), h('td', { class: 'dim', style: 'word-break:break-all' }, l.detail)))) : h('p', { class: 'dim' }, 'Nothing yet.'),
    h('h2', {}, 'Events a plugin can listen to'), h('p', {}, r.events.map(e => h('code', { style: 'margin-right:6px' }, e))));
}

// ---------- admin: settings
async function settingsPage() {
  markNav('settings');
  const [r, tc] = await Promise.all([GET('/api/settings'), GET('/api/transcoder')]);
  const s = r.settings;
  const f = {};
  const note = t => t ? h('div', { class: 'dim', style: 'font-size:13px' }, t) : null;
  const inp = (k, label, type, help) => [h('label', {}, label), (f[k] = h('input', { type: type || 'text', value: s[k], style: 'width:100%;max-width:420px' })), note(help)];
  const sel = (k, label, opts, help) => [h('label', {}, label), (f[k] = h('select', {}, opts.map(([v, l]) => h('option', { value: v, selected: String(s[k]) === String(v) }, l)))), note(help)];
  const chk = (k, label, help) => [h('label', { style: 'display:flex;gap:8px;align-items:center' }, (f[k] = h('input', { type: 'checkbox', checked: !!s[k] })), label), note(help)];
  set(h('h1', {}, 'Settings'),
    h('div', { class: 'panel' }, h('h2', { style: 'margin-top:0' }, 'Server'),
      inp('server_name', 'Server name'),
      inp('port', 'Port', 'number', `Default 8484. Takes effect after a restart; right now the server listens on ${r.port_in_use}. With Docker, change the port mapping in docker-compose.yml instead.`),
      inp('public_url', 'Address used in invite links', 'text', 'e.g. http://my-nas.example.com:8484 — leave empty to use the address you opened this page with'),
      chk('scheduled_scans_enabled', 'Scan my library periodically', 'Off by default. Turn it on so new files show up without pressing "Scan".'),
      sel('scan_interval_minutes', 'Library scan interval', r.scan_intervals.map(x => [x.minutes, x.label])),
      sel('watched_threshold_percent', 'Video played threshold', r.played_thresholds.map(p => [p, p + '%']), 'Set the progress percentage for video playback at which point the video will be marked as played.'),
      chk('watch_folders', 'Scan my library automatically', 'Your library will be updated automatically when files are added to, changed in or removed from library folders (checked every 15 seconds; a file is added once copying has finished).'),
      sel('credits_markers_behaviour', 'Generate credits video markers', r.credits_marker_choices.map(c => [c.value, c.label]), 'Detects movie and episode end credits: from chapters named "Credits" in the file, otherwise by looking at the end of the video. Can also be switched off per library; the Libraries page has a "Detect credits" button to run it now.'),
      chk('credits_from_picture', 'Look at the picture to find credits', 'When a file has no "Credits" chapter, the last part of the video is checked for the typical credits look (dark screen with text). Needs ffmpeg.'),
      sel('played_at_behaviour', 'Mark videos as watched', r.played_at_choices.map(c => [c.value, c.label]), 'Credits markers come from chapters named "Credits" / "Abspann" in the file, or are set by hand on the movie or episode page. Without markers the threshold is used.'),
      inp('continue_watching_weeks', 'Weeks to consider for Continue Watching', 'number'), inp('continue_watching_limit', 'Maximum Continue Watching items', 'number'),
      chk('continue_watching_include_premieres', 'Include season premieres in Continue Watching', 'New season premieres always appear no matter how many weeks have passed since watching.'),
      inp('invite_default_days', 'Invite links last (days)', 'number')),
    h('div', { class: 'panel' }, h('h2', { style: 'margin-top:0' }, 'Movie and show details'),
      chk('metadata_enabled', 'Download plot, cast, posters and episode titles from the internet'),
      sel('metadata_language', 'Language of titles, plots and episode names', r.languages.map(l => [l.code, l.name]), 'Used by every library that is set to "Same as server" on the Libraries page (each library can have its own). Changing it downloads the details again in the background — watch the progress at the top of the page. Show episode names are only translated with a TMDB key.'),
      chk('imdb_ratings_file', 'Show IMDb ratings from IMDb’s ratings file', 'Downloads IMDb’s free ratings list (about 9 MB) once a week. Rotten Tomatoes and Metacritic scores come from Wikidata; the TMDB score needs a TMDB key.'),
      inp('tmdb_api_key', 'TMDB API key (optional)', 'text', 'Without a key, movies come from Wikipedia/Wikidata and TV shows from TVmaze. A free key from themoviedb.org gives better matches, cast photos and translated plots.'),
      inp('omdb_api_key', 'OMDb API key for IMDb plots (optional)', 'text', 'With a free key from omdbapi.com, movie and show plots are taken from IMDb (in English) instead of Wikipedia. Press Refresh details on a movie, or change the language, to load them for movies you already have.')),
    h('div', { class: 'panel' }, h('h2', { style: 'margin-top:0' }, 'Transcoder'),
      h('p', { class: tc.ffmpeg_found ? 'ok' : 'err' }, tc.ffmpeg_found ? 'ffmpeg found: ' + tc.ffmpeg_version : 'ffmpeg was not found. Install it (the Docker image includes it) or enter its path below; without it only browser-friendly files play.'),
      tc.hardware_encoders.length ? h('p', { class: 'dim' }, 'Hardware encoders in this ffmpeg: ' + tc.hardware_encoders.join(', ') + (tc.render_device ? ' · /dev/dri/renderD128 present' : '')) : null,
      h('p', { class: 'dim' }, 'Converting with: ' + (tc.hw_in_use === 'none' ? 'the processor (software)' : tc.hw_in_use)),
      sel('transcoder_quality', 'Transcoder quality', [['automatic', 'Automatic'], ['speed', 'Prefer higher speed encoding'], ['quality', 'Prefer higher quality encoding'], ['max', 'Make my CPU hurt']], 'Applies to Intel Quick Sync and NVIDIA hardware encoding.'),
      sel('hw_accel', 'Hardware acceleration', [['auto', 'Automatic (use the graphics chip when it works)'], ['none', 'Off (software)'], ['vaapi', 'Intel/AMD VAAPI (most Synology Plus models)'], ['qsv', 'Intel Quick Sync'], ['nvenc', 'NVIDIA']]),
      chk('hw_constant_quality', 'Graphics chip: convert by quality instead of bitrate', 'Off: the graphics chip gets a fixed bitrate (safe on every model). On: it gets a quality number from 15 (best) to 25, like the software encoder does. Try it if hardware-converted video looks blocky.'),
      sel('h264_preset', 'Software encoder speed (x264 preset)', r.h264_presets.map(p => [p, p])),
      inp('h264_minimum_crf', 'Best allowed x264 quality (minimum CRF)', 'number', 'Lower = better picture and bigger stream. The quality the player asks for is mapped between this and 24.'),
      inp('max_transcodes', 'Maximum simultaneous video transcodes (0 = no limit)', 'number'),
      inp('hls_segment_seconds', 'HLS segment length (seconds)', 'number'), inp('segmented_transcoder_timeout_seconds', 'Segmented transcoder timeout (seconds)', 'number'),
      inp('ffmpeg', 'ffmpeg program'), inp('ffprobe', 'ffprobe program')),
    h('div', { class: 'actions' }, h('button', { class: 'primary', onclick: async () => {
      const body = {};
      for (const [k, el] of Object.entries(f)) body[k] = typeof s[k] === 'boolean' ? el.checked : typeof s[k] === 'number' ? Number(el.value) : el.value;
      try {
        const x = await api('PUT', '/api/settings', body);
        toast(['Saved', x.refetching ? `details for ${x.refetching} movies and shows are being downloaded again in the new language` : '', x.restart_needed || ''].filter(Boolean).join(' — '), x.restart_needed || x.refetching ? 8000 : 2600);
        boot();
      } catch (e) { toast(e.message, 5000); }
    } }, 'Save settings')),
    h('div', { class: 'panel' }, h('h2', { style: 'margin-top:0' }, 'Your account'),
      h('button', { onclick: async () => { const old = prompt('Current password'); if (!old) return; const nw = prompt('New password (at least 6 characters)'); if (!nw) return; try { await api('POST', '/api/me/password', { old, new: nw }); toast('Password changed'); } catch (e) { toast(e.message); } } }, 'Change my password'),
      h('p', { class: 'dim' }, 'Data folder on the server: ', h('code', {}, r.data_dir))));
}

// ---------- API page
async function apiPage() {
  markNav('api');
  const d = await GET('/api/docs');
  const shown = h('code', {}, '••••••••');
  const search = h('input', { type: 'search', placeholder: 'Find an address…', style: 'width:260px' });
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
    const t = await GET('/api/tokens');
    tokens.replaceChildren(h('table', {}, t.tokens.map(k => h('tr', {},
      h('td', {}, h('code', {}, k.id)), h('td', {}, k.device || 'Web app', k.current ? h('span', { class: 'badge' }, 'this one') : ''),
      h('td', { class: 'dim' }, new Date(k.created_at * 1000).toLocaleString()),
      h('td', {}, k.current ? '' : h('button', { class: 'small', onclick: async () => { await api('DELETE', '/api/tokens/' + k.id); toast('Token revoked'); loadTokens(); } }, 'Revoke'))))));
  };
  const rotate = async all => {
    if (!confirm(all ? 'Make a new token and sign out every other device and script?' : 'Make a new token? Scripts using the old one stop working.')) return;
    const r = await api('POST', '/api/tokens/rotate', { all });
    S.token = r.token; localStorage.setItem('token', r.token); shown.textContent = '••••••••';
    toast(all ? `New token made; ${r.revoked - 1} other sign-in(s) ended` : 'New token made; the old one no longer works');
    loadTokens();
  };
  set(h('h1', {}, 'API'),
    h('p', {}, 'Everything this app does goes through this JSON API, so other apps and scripts can do the same. ', d.auth, '.'),
    h('div', { class: 'panel' }, 'Your token: ', shown, ' ',
      h('button', { class: 'small', onclick: () => { shown.textContent = S.token; } }, 'Show'), ' ',
      h('button', { class: 'small', onclick: () => rotate(false) }, 'Rotate token'), ' ',
      h('button', { class: 'small', onclick: () => rotate(true) }, 'Rotate and sign out everywhere else'), ' ',
      h('button', { class: 'small', onclick: async () => { const n = prompt('Name for the new token (e.g. "backup script")'); if (n === null) return; const r = await api('POST', '/api/tokens', { device: n }); prompt('Copy the new token now; it is not shown again:', r.token); loadTokens(); } }, 'New token for a script'),
      h('p', { class: 'dim' }, 'Example: ', h('code', {}, `curl -H "X-Token: <token>" ${location.origin}/library/sections`)),
      tokens),
    h('div', { class: 'panel' }, h('b', {}, 'Filters'), h('p', { class: 'dim' }, 'Browsing, playlist contents and smart playlists share the same fields: ', S.filters.fields.map(f => h('code', { style: 'margin-right:5px' }, f.field)), '. In addresses: ', Object.entries(S.filters.url_shortcuts).map(([k, v]) => h('span', {}, h('code', {}, k), ` ${v}; `)))),
    h('div', { style: 'display:flex;gap:8px;flex-wrap:wrap;align-items:center;margin:10px 0' }, search,
      h('button', { class: 'small', onclick: () => groups.forEach(g => g.box.open = true) }, 'Open all'),
      h('button', { class: 'small', onclick: () => groups.forEach(g => g.box.open = false) }, 'Close all')),
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
      h('div', { class: 'info' }, h('h1', {}, r.name || 'Unknown'), h('p', { class: 'dim' }, `In ${r.size} ${r.size === 1 ? 'movie or show' : 'movies and shows'} in your libraries`))),
    r.Metadata.length ? h('div', { class: 'grid' }, r.Metadata.map(it => h('div', {}, posterCard(it), it.role ? h('div', { class: 'dim', style: 'font-size:12px;margin-top:2px' }, 'as ' + it.role) : null)))
      : h('div', { class: 'empty' }, 'Nothing in your libraries.'));
}

// ---------- router
async function render() {
  const hash = location.hash.slice(1) || '/';
  const [path, qs] = hash.split('?');
  const parts = path.split('/').filter(Boolean);
  if (!S.status) { try { S.status = await GET('/api/status'); } catch (e) { return set(h('div', { class: 'empty' }, 'The server cannot be reached.')); } }
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
      case 'login': location.hash = '#/'; return;
      default: return set(h('div', { class: 'empty' }, 'Page not found.'));
    }
  } catch (e) { set(h('div', { class: 'empty' }, 'Something went wrong: ' + e.message)); }
}
window.addEventListener('hashchange', () => { if (!P.it) render(); });
boot();
