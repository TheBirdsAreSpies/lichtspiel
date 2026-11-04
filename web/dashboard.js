// Lichtspiel web app, part 3: the watchlist and the owner's dashboard
// (what is playing, bandwidth, processor, memory, active users, play history).
'use strict';

// ---------- watchlist
async function toggleWatchlist(it, on = !it.watchlist) {
  await api('PUT', `/api/items/${it.id}/watchlist`, { watchlist: on });
  it.watchlist = on;
  toast(on ? t('Added to your watchlist') : t('Removed from your watchlist'));
  // On the watchlist itself a removed title leaves the page.
  if (!on && location.hash.startsWith('#/watchlist')) render();
}
async function watchlistPage(qs) {
  markNav('watchlist');
  const p = new URLSearchParams(qs || '');
  const st = { sort: p.get('sort') || '', type: p.get('type') || '' };
  const go = () => { const n = new URLSearchParams(); for (const [k, v] of Object.entries(st)) if (v) n.set(k, v); location.hash = '#/watchlist' + (n.toString() ? '?' + n : ''); };
  const items = meta(await GET(`/api/watchlist?sort=${st.sort}&type=${st.type}`));
  const tabs = [['', t('All')], ['movie', t('Movies')], ['show', t('Shows')], ['episode', t('Episodes')]];
  const sorts = [['', t('Added to the watchlist (newest)')], ['oldest', t('Added to the watchlist (oldest)')], ['title', t('Title')], ['year', t('Year')], ['rating', t('Rating')], ['added_at', t('Added to the library')]];
  set(h('h1', {}, t('Watchlist')),
    h('p', { class: 'dim', style: 'margin-top:-6px' }, t('What you want to watch later. A movie or episode leaves the list once you have watched it, a show once every episode is watched.')),
    h('div', { class: 'bar-tools' },
      h('div', { class: 'tabs', style: 'margin:0' }, tabs.map(([k, l]) => h('a', { href: '#', class: st.type === k ? 'on' : '', onclick: e => { e.preventDefault(); st.type = k; go(); } }, l))),
      h('span', { class: 'grow', style: 'flex:1' }),
      h('label', { style: 'margin:0;display:flex;gap:8px;align-items:center' }, t('Sort'),
        h('select', { onchange: e => { st.sort = e.target.value; go(); } }, sorts.map(([k, l]) => h('option', { value: k, selected: st.sort === k }, l)))),
      sizeSlider()),
    items.length ? pickGrid(items, { watchlistPage: true })
      : h('div', { class: 'empty' }, st.type ? t('Nothing of this kind on your watchlist.') : t('Your watchlist is empty. Use “Add to watchlist” on a movie or show, or in its “⋯” menu.')));
}

// ---------- dashboard
const DB = { timer: null, days: 30, user: '', metric: 'plays', hist: null, data: null, series: null, seriesAt: 0,
  range: store.get('dashRange', 'realtime'), cpuShow: store.get('dashCpu', 'all'), memShow: store.get('dashMem', 'all') };
const fmtBits = bps => { const b = bps * 8; return b >= 1e9 ? fmtNum(b / 1e9, 2) + ' Gbit/s' : b >= 1e6 ? fmtNum(b / 1e6, 1) + ' Mbit/s' : b >= 1e3 ? fmtNum(b / 1e3, 0) + ' kbit/s' : fmtNum(b, 0) + ' bit/s'; };
const fmtBytes = b => b >= 1e12 ? fmtNum(b / 1e12, 2) + ' TB' : b >= 1e9 ? fmtNum(b / 1e9, 1) + ' GB' : b >= 1e6 ? fmtNum(b / 1e6, 0) + ' MB' : fmtNum(b / 1e3, 0) + ' kB';
const fmtPct = v => v === null || v === undefined ? '–' : fmtNum(v, v < 10 ? 1 : 0) + ' %';
const fmtAgo = (sec, now) => {
  const d = Math.max(0, now - sec);
  if (d < 60) return t('just now');
  if (d < 3600) return t('{n} min ago', { n: Math.floor(d / 60) });
  if (d < 86400) return t('{n} h ago', { n: Math.floor(d / 3600) });
  return fmtDate(sec);
};
const fmtUptime = s => { const d = Math.floor(s / 86400), hh = Math.floor(s / 3600) % 24, mm = Math.floor(s / 60) % 60; return d ? t('{d} d {h} h', { d, h: hh }) : hh ? t('{h} h {m} min', { h: hh, m: mm }) : t('{m} min', { m: mm }); };
const KINDS = [['movie', 'Movies', 'var(--s1)'], ['episode', 'Episodes', 'var(--s2)'], ['other', 'Other videos', 'var(--s3)']];

function sv(tag, attrs, ...kids) {
  const el = document.createElementNS('http://www.w3.org/2000/svg', tag);
  for (const [k, v] of Object.entries(attrs || {})) if (v !== null && v !== undefined) el.setAttribute(k, v);
  for (const c of kids.flat(9)) if (c !== null && c !== undefined && c !== false) el.append(c instanceof Node ? c : document.createTextNode(String(c)));
  return el;
}
// One tooltip for every chart, kept next to the pointer and inside the window.
function vizTip(e, lines) {
  let tip = $('#viztip');
  if (!tip) { tip = h('div', { id: 'viztip', class: 'viztip', role: 'tooltip' }); document.body.append(tip); }
  if (!lines) { tip.style.display = 'none'; return; }
  tip.replaceChildren(...lines.map(([sw, label, val]) => h('div', { class: 'tl' }, sw ? h('i', { style: 'background:' + sw }) : null, h('span', {}, label), val !== undefined ? h('b', {}, val) : null)));
  tip.style.display = 'block';
  const w = tip.offsetWidth, ht = tip.offsetHeight;
  let x = e.clientX + 14, y = e.clientY + 14;
  if (x + w > innerWidth - 8) x = e.clientX - w - 14;
  if (y + ht > innerHeight - 8) y = e.clientY - ht - 14;
  tip.style.left = Math.max(8, x) + 'px'; tip.style.top = Math.max(8, y) + 'px';
}
const niceMax = v => { if (!(v > 0)) return 1; const p = Math.pow(10, Math.floor(Math.log10(v))); const n = v / p; return (n <= 1 ? 1 : n <= 2 ? 2 : n <= 2.5 ? 2.5 : n <= 5 ? 5 : 10) * p; };
// Four even steps of a round size up to at least v (0, 10, 20, 30, 40 for 33.4).
const niceTop = v => 4 * niceMax(Math.max(v, 1e-9) / 4);
const legend = entries => h('div', { class: 'legend' }, entries.map(([c, l]) => h('span', {}, h('i', { style: 'background:' + c }), l)));

// Lines over time (the last hour). series: [{name, color, values}], times: unix seconds.
// dates: the range is days long (the axis shows dates, the tooltip date and time).
function lineChart(box, { times, series, max, fmt, height = 150, dates = false }) {
  const when = (sec, tip) => { const d = new Date(sec * 1000); return dates ? (tip ? d.toLocaleString(I18N.lang, { day: 'numeric', month: 'numeric', year: 'numeric', hour: '2-digit', minute: '2-digit' }) : d.toLocaleDateString(I18N.lang, { day: 'numeric', month: 'numeric' })) : tip ? d.toLocaleTimeString(I18N.lang) : d.toLocaleTimeString(I18N.lang, { hour: '2-digit', minute: '2-digit' }); };
  const W = Math.max(260, box.clientWidth || 600), H = height, L = 52, R = 10, T = 10, B = 22;
  const all = series.flatMap(s => s.values).filter(v => v !== null && v !== undefined);
  const yMax = niceTop(max || Math.max(...all, 0) * 1.1 || 1);
  const n = times.length;
  const X = i => L + (n <= 1 ? 0 : (i / (n - 1)) * (W - L - R)), Y = v => T + (1 - v / yMax) * (H - T - B);
  const g = sv('svg', { viewBox: `0 0 ${W} ${H}`, width: '100%', height: H, class: 'viz', role: 'img' });
  for (let k = 0; k <= 4; k++) {
    const v = (yMax * k) / 4, y = Y(v);
    g.append(sv('line', { x1: L, x2: W - R, y1: y, y2: y, class: k ? 'grid' : 'axis' }), sv('text', { x: L - 6, y: y + 4, 'text-anchor': 'end', class: 'tick' }, fmt(v)));
  }
  if (n) {
    const labels = Math.min(5, n);
    let prev = '';
    for (let k = 0; k < labels; k++) {
      const i = Math.round((k / Math.max(1, labels - 1)) * (n - 1));
      const txt = when(times[i], false);
      if (txt === prev) continue;
      prev = txt;
      g.append(sv('text', { x: X(i), y: H - 6, 'text-anchor': k === 0 ? 'start' : k === labels - 1 ? 'end' : 'middle', class: 'tick' },
        txt));
    }
  }
  for (const s of series) {
    // A gap (no values: the server wasn't running) breaks the line; each piece gets its own fill.
    let d = '', area = '', seg = '', started = false, first = 0, lastI = 0;
    const closeSeg = () => { if (seg) area += seg + `L${X(lastI).toFixed(1)} ${Y(0)}L${X(first).toFixed(1)} ${Y(0)}Z`; seg = ''; };
    s.values.forEach((v, i) => {
      if (v === null || v === undefined) { if (started) closeSeg(); started = false; return; }
      const p = (started ? 'L' : 'M') + X(i).toFixed(1) + ' ' + Y(v).toFixed(1);
      d += p; seg += p;
      if (!started) first = i;
      started = true; lastI = i;
    });
    closeSeg();
    if (s.fill && area) g.append(sv('path', { d: area, fill: s.color, opacity: 0.14 }));
    if (d) g.append(sv('path', { d, fill: 'none', stroke: s.color, 'stroke-width': 2, 'stroke-linejoin': 'round', 'stroke-linecap': 'round' }));
  }
  if (!n) g.append(sv('text', { x: W / 2, y: H / 2, 'text-anchor': 'middle', class: 'tick' }, t('Collecting values…')));
  // Crosshair and tooltip.
  const cross = sv('line', { y1: T, y2: H - B, class: 'cross', style: 'display:none' });
  const dots = series.map(s => sv('circle', { r: 4, fill: s.color, stroke: 'var(--panel)', 'stroke-width': 2, style: 'display:none' }));
  g.append(cross, ...dots);
  const hit = sv('rect', { x: L, y: T, width: W - L - R, height: H - T - B, fill: 'transparent' });
  hit.addEventListener('mousemove', e => {
    if (!n) return;
    const r = g.getBoundingClientRect(), px = ((e.clientX - r.left) / r.width) * W;
    const i = Math.max(0, Math.min(n - 1, Math.round(((px - L) / (W - L - R)) * (n - 1))));
    cross.setAttribute('x1', X(i)); cross.setAttribute('x2', X(i)); cross.style.display = '';
    series.forEach((s, k) => { const v = s.values[i]; if (v === null || v === undefined) { dots[k].style.display = 'none'; return; } dots[k].setAttribute('cx', X(i)); dots[k].setAttribute('cy', Y(v)); dots[k].style.display = ''; });
    vizTip(e, [[null, when(times[i], true)], ...series.map(s => [s.color, s.name, s.values[i] === null || s.values[i] === undefined ? '–' : fmt(s.values[i], true)])]);
  });
  hit.addEventListener('mouseleave', e => { cross.style.display = 'none'; dots.forEach(d => (d.style.display = 'none')); vizTip(e, null); });
  g.append(hit);
  box.replaceChildren(g, series.length > 1 ? legend(series.map(s => [s.color, s.name])) : '');
}

// Stacked bars per day (or week). buckets: [{label, tip, parts: {movie, episode, other}}].
function barChart(box, { buckets, keys, fmt, height = 220 }) {
  const W = Math.max(280, box.clientWidth || 700), H = height, L = 44, R = 8, T = 10, B = 24;
  const totals = buckets.map(b => keys.reduce((a, [k]) => a + (b.parts[k] || 0), 0));
  const yMax = niceTop(Math.max(...totals, 0) || 1);
  const n = buckets.length, slot = (W - L - R) / Math.max(1, n), bw = Math.max(2, Math.min(28, slot * 0.72));
  const Y = v => T + (1 - v / yMax) * (H - T - B);
  const g = sv('svg', { viewBox: `0 0 ${W} ${H}`, width: '100%', height: H, class: 'viz', role: 'img' });
  for (let k = 0; k <= 4; k++) {
    const v = (yMax * k) / 4, y = Y(v);
    g.append(sv('line', { x1: L, x2: W - R, y1: y, y2: y, class: k ? 'grid' : 'axis' }), sv('text', { x: L - 6, y: y + 4, 'text-anchor': 'end', class: 'tick' }, fmt(v)));
  }
  const every = Math.ceil(n / Math.max(1, Math.floor((W - L - R) / 64)));
  buckets.forEach((b, i) => {
    const cx = L + slot * i + slot / 2, x = cx - bw / 2;
    if (i % every === 0) g.append(sv('text', { x: cx, y: H - 7, 'text-anchor': 'middle', class: 'tick' }, b.label));
    let base = 0;
    const segs = keys.filter(([k]) => b.parts[k] > 0);
    segs.forEach(([k, , color], si) => {
      const v = b.parts[k], y0 = Y(base), y1 = Y(base + v);
      // A 2 px gap between the parts; the top part gets rounded corners.
      const top = si === segs.length - 1, gap = si > 0 ? 2 : 0, hgt = Math.max(1, y0 - y1 - gap), r = top ? Math.min(4, bw / 2, hgt) : 0;
      const yt = y1, yb = y1 + hgt;
      const d = `M${x} ${yb}V${yt + r}Q${x} ${yt} ${x + r} ${yt}H${x + bw - r}Q${x + bw} ${yt} ${x + bw} ${yt + r}V${yb}Z`;
      g.append(sv('path', { d, fill: color }));
      base += v;
    });
    const hit = sv('rect', { x: L + slot * i, y: T, width: slot, height: H - T - B, fill: 'transparent' });
    hit.addEventListener('mousemove', e => { hit.setAttribute('class', 'hov'); vizTip(e, [[null, b.tip], ...keys.map(([k, l, c]) => [c, t(l), fmt(b.parts[k] || 0, true)]), [null, t('Together'), fmt(totals[i], true)]]); });
    hit.addEventListener('mouseleave', e => { hit.removeAttribute('class'); vizTip(e, null); });
    g.append(hit);
  });
  box.replaceChildren(g, legend(keys.map(([, l, c]) => [c, t(l)])));
}

const methodLabel = p => p.method === 'transcode' ? t('Transcode') + (p.height ? ` ${p.height}p` : '') : p.method === 'remux' ? t('Direct stream') : t('Direct play');
const CODECS = { hevc: 'HEVC', h264: 'H.264', av1: 'AV1', vp9: 'VP9', vp8: 'VP8', mpeg2video: 'MPEG-2', mpeg4: 'MPEG-4', vc1: 'VC-1', ac3: 'AC3', eac3: 'EAC3', dts: 'DTS', truehd: 'TrueHD', aac: 'AAC', flac: 'FLAC', opus: 'Opus', mp3: 'MP3', vorbis: 'Vorbis', pcm_s16le: 'PCM', pcm_s24le: 'PCM' };
const codecName = c => CODECS[c] || (c || '').toUpperCase();
const resName = hh => !hh ? '' : hh >= 2000 ? '4K' : hh >= 1000 ? '1080p' : hh >= 700 ? '720p' : hh >= 540 ? '576p' : hh >= 470 ? '480p' : 'SD';
const chName = n => ({ 1: 'Mono', 2: t('Stereo'), 3: '2.1', 6: '5.1', 7: '6.1', 8: '7.1' }[n] || (n ? n + ' ch' : ''));
// "1080p (HEVC Main 10)" → "720p (H.264) — Transcode (hw)", like the player sees it.
function videoLine(v) {
  if (!v) return null;
  const from = `${resName(v.height)} (${[codecName(v.codec), v.profile, v.hdr ? 'HDR' : ''].filter(Boolean).join(' ')})`;
  return v.action === 'transcode'
    ? [from, ' → ', h('span', {}, `${resName(v.out_height)} (${codecName(v.out_codec)}) — ${t('Transcode')}${v.hw ? ' (hw)' : ''}`)]
    : [from, h('span', { class: 'to' }, ' — ' + t('Direct'))];
}
function audioLine(a) {
  if (!a) return null;
  const lang = a.language ? langName(a.language) : a.title || t('Unknown');
  const from = `${lang} (${[codecName(a.codec), chName(a.channels)].filter(Boolean).join(' ')})`;
  return a.action === 'transcode'
    ? [from, ' → ', h('span', {}, `${codecName(a.out_codec)} ${chName(a.out_channels)} — ${t('Transcode')}`)]
    : [from, h('span', { class: 'to' }, ' — ' + t('Direct'))];
}
function stopDialog(p, title) {
  const msg = h('textarea', { rows: 3, maxlength: 500, style: 'width:100%', placeholder: t('Message to {name} (optional)', { name: p.user || '?' }) });
  const box = modal(t('Stop playback?'), [
    h('p', { class: 'dim' }, t('“{title}” stops for {name} right away. They see your message in the Lichtspiel web app.', { title, name: p.user || '?' })),
    msg,
    h('div', { class: 'actions' },
      h('button', { class: 'primary', onclick: async e => {
        e.target.disabled = true;
        try { await api('POST', '/api/dashboard/stop', { user: p.user_id, item: p.id, message: msg.value }); toast(t('Playback stopped')); box.remove(); pollActivity(); }
        catch (er) { e.target.disabled = false; toast(er.message, 6000); }
      } }, t('Stop playback')),
      h('button', { onclick: () => box.remove() }, t('Cancel')))]);
  msg.focus();
}
function playingCard(p) {
  const pct = p.duration ? Math.min(100, (p.offset / p.duration) * 100) : 0;
  const title = p.type === 'episode' && p.grandparentTitle ? p.grandparentTitle : p.title || t('(removed)');
  const sub = p.type === 'episode' ? `${t('S{n}', { n: p.parentIndex || 0 })} ${t('E{n}', { n: p.index || 0 })} · ${p.title}` : p.year || '';
  const where = (p.local ? t('Local') : t('Remote')) + (p.ip ? ` (${p.ip})` : '') + ' — ' + fmtBits(p.out_bps || 0);
  const rows = [[t('Device'), p.client || '–'], [t('Connection'), where], [t('Video'), videoLine(p.video)], [t('Sound'), audioLine(p.audio)]].filter(([, v]) => v);
  return h('div', { class: 'np' },
    h('a', { href: '#/item/' + p.id, class: 'np-img' }, h('img', { src: withTok(p.thumb), alt: '', loading: 'lazy' })),
    h('div', { class: 'np-body' },
      h('div', { class: 'np-head' },
        h('div', {}, h('a', { href: '#/item/' + p.id }, h('b', {}, title)), h('div', { class: 'dim' }, sub)),
        iconBtn('stop', t('Stop playback'), () => stopDialog(p, p.type === 'episode' ? `${title} · ${p.title}` : title))),
      h('div', { class: 'np-who' }, h('b', {}, p.user || '?'), h('span', { class: 'chip' + (p.method === 'transcode' ? ' warn' : ''), style: 'margin-left:8px' }, methodLabel(p))),
      h('div', { class: 'np-rows' }, rows.flatMap(([k, v]) => [h('span', {}, k), h('span', {}, v)])),
      h('div', { class: 'np-prog' }, h('span', {}, p.state === 'paused' ? '⏸ ' + t('Paused') : '▶ ' + t('Playing')),
        h('div', { class: 'pbar' }, h('i', { style: `width:${pct}%` })),
        h('span', { class: 'dim np-time', 'data-offset': p.offset, 'data-duration': p.duration || 0, 'data-playing': p.state === 'paused' ? '' : '1', 'data-at': Date.now() }, p.duration ? `${fmtTime(p.offset / 1000)} / ${fmtTime(p.duration / 1000)}` : ''))));
}
// The position of everything that plays moves on every second, between the server's updates.
setInterval(() => {
  document.querySelectorAll('.np-time[data-playing="1"]').forEach(el => {
    const dur = Number(el.dataset.duration); if (!dur) return;
    const off = Math.min(dur, Number(el.dataset.offset) + (Date.now() - Number(el.dataset.at)));
    el.textContent = `${fmtTime(off / 1000)} / ${fmtTime(dur / 1000)}`;
    const bar = el.parentElement.querySelector('.pbar i'); if (bar) bar.style.width = Math.min(100, off / dur * 100) + '%';
  });
}, 1000);
function tile(label, value, note, spark) {
  return h('div', { class: 'tile' }, h('div', { class: 'dim lbl' }, label), h('div', { class: 'val' }, value), note ? h('div', { class: 'dim note' }, note) : null, spark || null);
}

async function dashboardPage() {
  markNav('dashboard');
  if (!S.me.admin) return set(h('div', { class: 'empty' }, t('Only an admin can do this')));
  clearTimeout(DB.timer);
  const live = h('div', {}), charts = h('div', { class: 'dash-charts' }), hist = h('div', {});
  set(h('div', { class: 'dash-head' }, h('h1', {}, t('Dashboard')), h('span', { class: 'dim', id: 'dashsub' })), live, chartTools(charts), charts, hist);
  await Promise.all([dashLive(live, charts), dashHistory(hist)]);
}
async function dashLive(live, charts) {
  clearTimeout(DB.timer);
  if (!live.isConnected) return;
  let d;
  try { d = await GET('/api/dashboard'); } catch (e) { live.replaceChildren(h('div', { class: 'empty' }, e.message)); DB.timer = setTimeout(() => dashLive(live, charts), 10000); return; }
  if (!live.isConnected) return;
  DB.data = d;
  drawPlayingBtn(d.playing.length);
  const c = d.current || {};
  const active = d.users.filter(u => u.active);
  const tc = d.playing.filter(p => p.method === 'transcode').length;
  const sub = $('#dashsub');
  if (sub) sub.textContent = t('Running for {time} · version {v} · {sent} sent since the start · updates every {n} s', { time: fmtUptime(d.now - d.started_at), v: d.version, sent: fmtBytes(d.bytes_sent), n: d.sample_seconds });
  const mem = c.mem_total ? `${fmtBytes(c.mem_used)} / ${fmtBytes(c.mem_total)}` : '–';
  live.replaceChildren(
    h('div', { class: 'tiles' },
      tile(t('Playing now'), String(d.playing.length), tc ? t('{n} of them converted', { n: tc }) + (d.max_transcodes ? ' ' + t('(at most {n} at once)', { n: d.max_transcodes }) : '') : t('none converted')),
      tile(t('Bandwidth'), fmtBits(c.out_bps || 0), c.out_bps ? t('{local} local · {remote} remote', { local: fmtBits(Math.max(0, c.out_bps - (c.out_remote_bps || 0))), remote: fmtBits(c.out_remote_bps || 0) }) : t('sent to players right now')),
      tile(t('Processor'), fmtPct(c.cpu), c.cpu_server !== null && c.cpu_server !== undefined ? t('Lichtspiel: {pct} · {n} cores', { pct: fmtPct(c.cpu_server), n: d.cores }) : t('{n} cores', { n: d.cores })),
      tile(t('Memory'), c.mem_total ? fmtPct((100 * c.mem_used) / c.mem_total) : '–', mem + (c.mem_server ? ' · ' + t('Lichtspiel: {size}', { size: fmtBytes(c.mem_server) }) : '')),
      tile(t('Active users'), `${active.length}`, t('of {n} · active = seen in the last {m} min', { n: d.users.length, m: Math.round(d.active_seconds / 60) }))),
    h('h2', {}, t('Playing now')),
    d.playing.length ? h('div', { class: 'np-list' }, d.playing.map(playingCard)) : h('div', { class: 'empty', style: 'padding:22px' }, t('Nothing is playing right now.')),
    h('h2', {}, t('Users')),
    h('div', { class: 'panel', style: 'padding:6px 12px;overflow-x:auto' }, h('table', {},
      h('tr', {}, [t('User'), t('Status'), t('Last seen'), t('Device'), t('Address'), t('Playing'), t('Bandwidth'), t('Last viewing')].map(x => h('th', {}, x))),
      d.users.map(u => h('tr', {},
        h('td', {}, h('b', {}, u.username), u.admin ? h('span', { class: 'tag', style: 'margin-left:6px' }, t('Admin')) : null),
        h('td', {}, h('span', { class: 'status ' + (u.active ? 'on' : 'off') }, h('i', {}), u.active ? t('active') : t('away'))),
        h('td', { class: 'dim' }, u.last_seen ? fmtAgo(u.last_seen, d.now) : t('not since the start')),
        h('td', { class: 'dim' }, u.client || '–'), h('td', { class: 'dim' }, u.ip || '–'),
        h('td', {}, u.playing ? String(u.playing) : '–'), h('td', {}, u.out_bps ? fmtBits(u.out_bps) : '–'),
        h('td', { class: 'dim' }, u.last_play ? fmtDateTime(u.last_play) : '–'))))));
  DB.timer = setTimeout(() => dashLive(live, charts), (d.sample_seconds || 5) * 1000);
  // Real time follows every sample; longer ranges change slowly (once a minute is enough).
  if (DB.range === 'realtime' || !DB.series || DB.series.range !== DB.range || Date.now() - DB.seriesAt > 60000) await loadSeries();
  if (live.isConnected) drawCharts(charts);
}
async function loadSeries() {
  try { DB.series = await GET('/api/dashboard/stats?range=' + DB.range); DB.seriesAt = Date.now(); } catch (e) { /* the next round tries again */ }
}
const RANGES = [['realtime', 'Real time'], ['12h', 'Last 12 hours'], ['24h', 'Last 24 hours'], ['7d', 'Last 7 days'], ['30d', 'Last 30 days'], ['90d', 'Last 90 days'], ['365d', 'Last year'], ['all', 'Unlimited']];
const SHOW = [['all', 'All'], ['server', 'Lichtspiel'], ['system', 'System']];
function segBtns(vals, cur, pick) {
  return h('div', { class: 'seg' }, vals.map(([v, l]) => h('button', { class: 'small' + (cur === v ? ' on' : ''), onclick: () => pick(v) }, t(l))));
}
// The time range for every chart, above them.
function chartTools(charts) {
  const box = h('div', { class: 'dash-head', style: 'margin-top:26px' }, h('h2', { style: 'margin:0' }, t('Activity')),
    h('select', { 'aria-label': t('Time range'), onchange: async e => {
      DB.range = e.target.value; store.set('dashRange', DB.range);
      await loadSeries(); drawCharts(charts);
    } }, RANGES.map(([v, l]) => h('option', { value: v, selected: DB.range === v }, t(l)))));
  return box;
}
function drawCharts(charts) {
  const r = DB.series, c = (DB.data && DB.data.current) || {};
  if (!r || !charts.isConnected) return;
  const ss = r.samples || [];
  const times = ss.map(s => s.at);
  // Longer than a day and a half: the axis shows days.
  const dates = ss.length > 1 && times[times.length - 1] - times[0] > 36 * 3600;
  const label = t((RANGES.find(x => x[0] === r.range) || RANGES[0])[1]);
  const num = v => (v === null || v === undefined ? null : v);
  const gb = v => (v === null || v === undefined ? null : v / 1e9);
  const memMax = c.mem_total ? c.mem_total / 1e9 : null;
  const box = (title, ...side) => { const b = h('div', { class: 'chart' }); return [h('div', { class: 'panel' }, h('div', { class: 'ch-title' }, h('b', {}, title), h('span', { class: 'ch-side' }, side)), b), b]; };
  const redraw = () => drawCharts(charts);
  // The unit follows the highest value of the range (bit/s, kbit/s, Mbit/s, Gbit/s).
  const peak = Math.max(0, ...ss.map(s => (s.out_bps || 0) * 8));
  const [div, unit] = peak >= 1e9 ? [1e9, 'Gbit/s'] : peak >= 1e6 ? [1e6, 'Mbit/s'] : peak >= 1e3 ? [1e3, 'kbit/s'] : [1, 'bit/s'];
  const [p1, b1] = box(t('Bandwidth'), h('span', { class: 'dim' }, label + ' · ' + unit));
  const [p2, b2] = box(t('Processor'), segBtns(SHOW, DB.cpuShow, v => { DB.cpuShow = v; store.set('dashCpu', v); redraw(); }));
  const [p3, b3] = box(t('Memory'), segBtns(SHOW, DB.memShow, v => { DB.memShow = v; store.set('dashMem', v); redraw(); }));
  const [p4, b4] = box(t('Streams'), h('span', { class: 'dim' }, label));
  charts.replaceChildren(p1, p2, p3, p4);
  const bits = v => (v === null || v === undefined ? null : (v * 8) / div);
  lineChart(b1, { times, dates, series: [
    { name: t('Local'), color: 'var(--s1)', values: ss.map(s => (s.out_bps === undefined ? null : bits(Math.max(0, s.out_bps - (s.out_remote_bps || 0))))), fill: true },
    { name: t('Remote'), color: 'var(--s2)', values: ss.map(s => (s.out_bps === undefined ? null : bits(s.out_remote_bps || 0))), fill: true }],
  fmt: (v, full) => fmtNum(v, v && v < 10 && v % 1 ? 1 : 0) + (full ? ' ' + unit : '') });
  const pick = (show, sys, srv) => [show !== 'server' ? sys : null, show !== 'system' ? srv : null].filter(Boolean);
  lineChart(b2, { times, dates, max: DB.cpuShow === 'server' ? null : 100, series: pick(DB.cpuShow,
    { name: t('System'), color: 'var(--s1)', values: ss.map(s => num(s.cpu)) },
    { name: 'Lichtspiel', color: 'var(--s2)', values: ss.map(s => num(s.cpu_server)) }), fmt: v => fmtNum(v, 0) + ' %' });
  lineChart(b3, { times, dates, max: DB.memShow === 'server' ? null : memMax, series: pick(DB.memShow,
    { name: t('System'), color: 'var(--s1)', values: ss.map(s => gb(s.mem_used)) },
    { name: 'Lichtspiel', color: 'var(--s2)', values: ss.map(s => gb(s.mem_server)) }), fmt: (v, full) => fmtNum(v, full && v < 10 ? 1 : 0) + (full ? ' GB' : '') });
  lineChart(b4, { times, dates, series: [{ name: t('Playing'), color: 'var(--s1)', values: ss.map(s => num(s.streams)) }, { name: t('Converted'), color: 'var(--s2)', values: ss.map(s => num(s.transcodes)) }], max: Math.max(4, ...ss.map(s => s.streams || 0)), fmt: v => fmtNum(v, 0) });
}
// Viewings per day (or per week for long spans), in this browser's time zone.
async function dashHistory(box) {
  const q = `/api/dashboard/history?days=${DB.days}${DB.user ? '&user=' + DB.user : ''}`;
  let r;
  try { r = await GET(q); } catch (e) { return box.replaceChildren(h('div', { class: 'empty' }, e.message)); }
  DB.hist = r;
  const weekly = DB.days > 90;
  const day0 = new Date(); day0.setHours(0, 0, 0, 0);
  const start = new Date(day0); start.setDate(start.getDate() - (DB.days - 1));
  if (weekly) { const wd = (start.getDay() + 6) % 7; start.setDate(start.getDate() - wd); }
  const buckets = [];
  for (const d = new Date(start); d <= day0; d.setDate(d.getDate() + (weekly ? 7 : 1))) {
    const from = new Date(d), to = new Date(d); to.setDate(to.getDate() + (weekly ? 7 : 1));
    const short = from.toLocaleDateString(I18N.lang, { day: 'numeric', month: 'numeric' });
    buckets.push({ from: from.getTime() / 1000, to: to.getTime() / 1000, label: short, parts: { movie: 0, episode: 0, other: 0 },
      tip: weekly ? t('Week of {date}', { date: from.toLocaleDateString(I18N.lang) }) : from.toLocaleDateString(I18N.lang, { weekday: 'long', day: 'numeric', month: 'long', year: 'numeric' }) });
  }
  const hours = DB.metric === 'hours';
  for (const [at, , kind, mins] of r.plays) {
    const b = buckets.find(x => at >= x.from && at < x.to); if (!b) continue;
    b.parts[kind] += hours ? mins / 60 : 1;
  }
  const users = (DB.data && DB.data.users) || [];
  const fmt = (v, full) => hours ? fmtNum(v, v && v < 10 ? 1 : 0) + (full ? ' h' : '') : fmtNum(Math.round(v * 100) / 100, v % 1 ? 1 : 0);
  const seg = (vals, cur, pick) => h('div', { class: 'seg' }, vals.map(([v, l]) => h('button', { class: 'small' + (cur === v ? ' on' : ''), onclick: () => { pick(v); dashHistory(box); } }, l)));
  const chart = h('div', { class: 'chart' });
  const ranked = (list, f) => list.length ? h('ol', { class: 'ranked' }, list.map(f)) : h('p', { class: 'dim' }, t('No viewings in this time.'));
  const what = it => it.type === 'episode' && it.grandparentTitle ? `${it.grandparentTitle} · ${t('S{n}', { n: it.parentIndex || 0 })}${t('E{n}', { n: it.index || 0 })} ${it.title}` : it.title || t('(removed)');
  box.replaceChildren(
    h('div', { class: 'dash-head', style: 'margin-top:26px' }, h('h2', { style: 'margin:0' }, t('Play history')),
      h('div', { class: 'hist-tools' },
        seg([[7, t('7 days')], [30, t('30 days')], [90, t('90 days')], [365, t('1 year')]], DB.days, v => (DB.days = v)),
        seg([['plays', t('Viewings')], ['hours', t('Hours')]], DB.metric, v => (DB.metric = v)),
        h('select', { onchange: e => { DB.user = e.target.value; dashHistory(box); } }, h('option', { value: '' }, t('Everyone')),
          users.map(u => h('option', { value: u.id, selected: String(u.id) === String(DB.user) }, u.username))))),
    h('div', { class: 'panel' },
      h('div', { class: 'ch-title' }, h('b', {}, hours ? t('Hours watched per {unit}', { unit: weekly ? t('week') : t('day') }) : t('Viewings per {unit}', { unit: weekly ? t('week') : t('day') })),
        h('span', { class: 'dim' }, t('{n} viewings · {h} watched', { n: r.size, h: fmtDur(r.minutes * 60000) || '0 min' }))),
      chart),
    h('div', { class: 'dash-cols' },
      h('div', { class: 'panel' }, h('b', {}, t('Most played')),
        ranked(r.titles, x => h('li', {}, h('img', { src: withTok(x.thumb), alt: '', loading: 'lazy' }),
          h('div', { class: 'grow' }, h('a', { href: '#/item/' + x.id }, x.title || t('(removed)')), h('div', { class: 'dim' }, [x.year, x.type === 'show' ? t('Show') : null].filter(Boolean).join(' · '))),
          h('span', { class: 'dim' }, t('{n}×', { n: x.plays }))))),
      h('div', { class: 'panel' }, h('b', {}, t('Top users')),
        ranked(r.users, x => h('li', {}, h('div', { class: 'grow' }, h('b', {}, x.username || '?')), h('span', { class: 'dim' }, t('{n} viewings · {h} watched', { n: x.plays, h: fmtDur(x.minutes * 60000) || '0 min' })))))),
    h('div', { class: 'panel', style: 'padding:6px 12px;overflow-x:auto' }, h('b', { style: 'display:block;padding:8px 0' }, t('Latest viewings')),
      r.recent.length ? h('table', {}, h('tr', {}, [t('When'), t('User'), t('Title'), ''].map(x => h('th', {}, x))),
        r.recent.map(e => h('tr', {}, h('td', { class: 'dim' }, fmtDateTime(e.at)), h('td', {}, e.user || '?'),
          h('td', {}, e.item.title ? h('a', { href: '#/item/' + e.item.id }, what(e.item)) : what(e.item)),
          h('td', { class: 'dim' }, e.manual ? t('marked by hand') : [fmtDur(e.minutes * 60000), e.live ? ' · ' + t('playing now') : ''])))) : h('p', { class: 'dim' }, t('No viewings in this time.'))));
  barChart(chart, { buckets, keys: KINDS, fmt });
}
window.addEventListener('hashchange', () => { if (!location.hash.startsWith('#/dashboard')) clearTimeout(DB.timer); vizTip({}, null); });
let dashResize;
window.addEventListener('resize', () => {
  clearTimeout(dashResize);
  dashResize = setTimeout(() => { if (location.hash.startsWith('#/dashboard') && DB.hist) { const hb = $('#main').lastChild; if (hb) dashHistory(hb); } }, 250);
});
