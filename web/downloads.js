// Lichtspiel web app, part 4: downloads. Pick a quality for a movie, an
// episode, a season, a show or several posters; the original is ready at
// once, other qualities are converted on the server first. The Downloads
// page shows each one with its progress and a button to save it.
'use strict';

const DL = { timer: null, all: false };
const DL_QUALITIES = () => QUALITIES.filter(q => q[0] !== '1080p-20' && q[0] !== '1080p-12');
const dlQualityName = q => (QUALITIES.find(x => x[0] === q) || [q, q])[1];
// About how big a converted copy gets: picture plus 160 kbit/s stereo sound.
const dlEstimate = (q, ms) => { const m = /^(\d+)p-([\d.]+)$/.exec(q); return m && ms ? (Number(m[2]) * 1000 + 160) * 125 * ms / 1000 : 0; };

function saveFile(url) {
  // A hidden link: the browser saves the file and the page stays.
  const a = h('a', { href: withTok(url), download: '', style: 'display:none' });
  document.body.append(a); a.click(); a.remove();
}

// items: one or several movies, episodes, seasons or shows.
async function downloadDialog(items) {
  if (!S.me.can_download) return toast(t('Downloads not allowed'), 5000);
  const one = items.length === 1 ? await fullItem(items[0]) : null;
  const playable = one && isPlayable(one);
  const media = playable && one.Media ? one.Media[0] : null;
  const leaves = items.reduce((n, it) => n + (isPlayable(it) ? 1 : it.leafCount || 0), 0);
  const ms = items.reduce((n, it) => n + (it.duration || 0) * (isPlayable(it) ? 1 : 0), 0);
  const mayConvert = S.me.admin || S.me.allow_transcode !== false;
  let last = 'original';
  try { last = localStorage.getItem('dlQuality') || 'original'; } catch (e) { /* private window */ }
  if (!mayConvert) last = 'original';
  const label = ([q, l]) => {
    const orig = media ? media.size : 0;
    // Never more than the original: a file that already fits is sent as it is.
    const size = q === 'original' ? orig : orig ? Math.min(orig, dlEstimate(q, ms)) : dlEstimate(q, ms);
    return size ? `${l} · ${q === 'original' ? '' : '≈ '}${fmtSize(size)}` : l;
  };
  const sel = h('select', {}, DL_QUALITIES().map(x => h('option', { value: x[0], selected: x[0] === last, disabled: x[0] !== 'original' && !mayConvert }, label(x))));
  const title = one ? (one.type === 'episode' && one.grandparentTitle ? `${one.grandparentTitle} · ${one.title}` : one.type === 'season' ? `${one.parentTitle || ''} · ${seasonTitle(one.title)}` : one.title) : t('{n} selected', { n: items.length });
  const note = h('p', { class: 'dim' });
  const explain = () => note.textContent = sel.value === 'original'
    ? t('The file as it is on the server. It is ready right away.')
    : t('Converted on the server to MP4 (H.264, stereo sound) that plays on every device. It is listed on the Downloads page with its progress; save it from there once it is ready. A file that is already small enough is sent as it is.');
  sel.onchange = explain; explain();
  const box = modal(t('Download “{title}”', { title }), [
    !playable && leaves ? h('p', {}, leaves === 1 ? t('1 video') : t('{n} videos', { n: leaves })) : null,
    formRow(t('Quality'), sel),
    note,
    !mayConvert ? h('p', { class: 'dim' }, t('You may not have videos converted, so only the original quality can be downloaded.')) : null,
    h('div', { class: 'actions' },
      h('button', { class: 'primary', onclick: async () => {
        const q = sel.value;
        try { localStorage.setItem('dlQuality', q); } catch (e) { /* private window */ }
        let r;
        try { r = await api('POST', '/api/downloads', { ids: items.map(i => i.id), quality: q }); } catch (e) { return toast(e.message, 6000); }
        box.remove();
        // One movie or episode in its original quality: saved right away.
        if (playable && r.ready === 1 && r.added === 1) {
          const list = await GET('/api/downloads');
          const d = list.downloads.find(x => x.item && x.item.id === one.id && x.quality === q);
          if (d && d.parts <= 1) { saveFile(d.url); toast(t('Saving the file — it is also on the Downloads page')); drawDlChip(); return; }
        }
        const parts = [];
        if (r.ready) parts.push(t('{n} ready', { n: r.ready }));
        if (r.queued) parts.push(t('{n} being converted', { n: r.queued }));
        if (r.already) parts.push(t('{n} already there', { n: r.already }));
        if (r.refused) parts.push(t('{n} left out (you may not convert)', { n: r.refused }));
        toast(t('Downloads: {what}', { what: parts.join(', ') }), 5000);
        drawDlChip();
      } }, t('Download')),
      h('a', { href: '#/downloads', onclick: () => box.remove() }, h('button', {}, t('Open Downloads'))),
      h('button', { onclick: () => box.remove() }, t('Cancel')))]);
  sel.focus();
}

// The number of unfinished downloads next to "Downloads" in the left menu.
async function drawDlChip(list) {
  const link = $('#side a[data-nav="downloads"]');
  if (!link || !S.me) return;
  link.style.display = S.me.can_download ? '' : 'none';
  if (!S.me.can_download) return;
  try { list = list || await GET('/api/downloads'); } catch (e) { return; }
  let chip = link.querySelector('small');
  const busy = list.queued + list.converting;
  if (!busy) { if (chip) chip.remove(); return; }
  if (!chip) { chip = h('small', { class: 'dlcount' }); link.append(chip); }
  chip.textContent = busy;
  chip.title = t('{n} being converted', { n: busy });
}

const DL_STATES = { queued: 'Waiting', converting: 'Converting', ready: 'Ready', failed: 'Failed' };
async function downloadsPage() {
  markNav('downloads');
  clearTimeout(DL.timer);
  const summary = h('span', { class: 'dim' }), tools = h('span', {}), listBox = h('div'), intro = h('p', { class: 'dim', style: 'margin-top:-6px' });
  const name = d => {
    const x = d.item || {};
    if (!x.title) return t('(no longer in the library)');
    return x.type === 'episode' ? `${x.grandparentTitle || ''} · ${t('S{n}', { n: x.parentIndex || 0 })}${t('E{n}', { n: x.index || 0 })} ${x.title}` : x.title;
  };
  const row = d => h('div', { class: 'li' },
    h('img', { src: withTok(d.thumb), loading: 'lazy', alt: '' }),
    h('div', { class: 'grow' },
      d.item && d.item.title ? h('a', { href: '#/item/' + d.item.id }, h('b', {}, name(d))) : h('b', {}, name(d)),
      h('div', { class: 'dim' }, [dlQualityName(d.quality), d.original && d.quality !== 'original' ? t('sent as it is (already fits)') : null,
        t(DL_STATES[d.state] || d.state) + (d.state === 'converting' ? ` ${d.progress} %` : ''),
        d.size ? fmtSize(d.size) : null, DL.all && d.user ? d.user : null,
        d.expires_at ? t('kept until {date}', { date: fmtDate(d.expires_at) }) : null].filter(Boolean).join(' · ')),
      d.state === 'converting' || d.state === 'queued' ? h('div', { class: 'dlbar' }, h('i', { style: `width:${d.progress}%` })) : null,
      d.state === 'failed' && d.error ? h('div', { class: 'err', style: 'font-size:13px;word-break:break-word' }, t(d.error)) : null),
    d.state === 'ready' && d.parts > 1 ? [...Array(d.parts).keys()].map(n => h('button', { class: 'small primary', onclick: () => saveFile(`${d.url}?part=${n}`) }, t('Save part {n}', { n: n + 1 })))
      : d.state === 'ready' ? h('button', { class: 'small primary', onclick: () => saveFile(d.url) }, t('Save')) : null,
    d.state === 'failed' ? h('button', { class: 'small', onclick: async () => { await api('POST', `/api/downloads/${d.id}/retry`); refresh(); } }, t('Try again')) : null,
    h('button', { class: 'small', title: d.state === 'converting' ? t('Stop and remove') : t('Remove'), 'aria-label': t('Remove'), onclick: async () => {
      if (d.state === 'converting' && !(await sure(t('Stop converting “{title}” and remove it?', { title: name(d) }), { ok: t('Remove'), danger: true }))) return;
      await api('DELETE', '/api/downloads/' + d.id); refresh();
    } }, '✕'));
  // Draws the list again in place (no jump to the top), every 2 s while something converts.
  const refresh = async () => {
    clearTimeout(DL.timer);
    const list = await GET('/api/downloads' + (DL.all ? '?all=1' : ''));
    if (!listBox.isConnected) return;
    drawDlChip(list);
    const busy = list.converting + list.queued;
    intro.replaceChildren(...(list.allowed
      ? [t('Use “Download…” on a movie, an episode, a season or a show, or in a poster’s “⋯” menu.'), ' ', list.keep_days ? t('Converted copies are kept on the server for {n} days.', { n: list.keep_days }) : '']
      : [t('Downloads are switched off for your account. Ask the owner of the server.')]));
    summary.textContent = [busy ? t('{n} being converted', { n: busy }) : null, t('{n} ready', { n: list.ready }), list.failed ? t('{n} failed', { n: list.failed }) : null].filter(Boolean).join(' · ');
    tools.replaceChildren(list.ready + list.failed ? h('button', { class: 'small', onclick: async () => {
      const r = await api('DELETE', '/api/downloads?state=finished' + (DL.all ? '&all=1' : '')); toast(t('{n} removed', { n: r.removed })); refresh();
    } }, t('Clear finished')) : '');
    listBox.replaceChildren(list.downloads.length ? h('div', { class: 'list' }, list.downloads.map(row)) : h('div', { class: 'empty' }, t('Nothing downloaded yet.')));
    if (busy) DL.timer = setTimeout(() => { if (listBox.isConnected && !document.querySelector('.modal')) refresh().catch(() => {}); }, 2000);
  };
  set(h('h1', {}, t('Downloads')), intro,
    h('div', { class: 'bar-tools' }, summary, h('span', { class: 'grow' }),
      S.me.admin ? h('label', { style: 'margin:0;display:flex;gap:6px;align-items:center' },
        h('input', { type: 'checkbox', checked: DL.all, onchange: e => { DL.all = e.target.checked; refresh(); } }), t('Everyone’s downloads')) : null,
      tools),
    listBox);
  await refresh();
}
window.addEventListener('hashchange', () => clearTimeout(DL.timer));
