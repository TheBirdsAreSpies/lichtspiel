// Lichtspiel web app: the player. Loaded after app.js and library.js.
'use strict';

// Own controls over the <video>: one seek bar for the whole video (also while converting, where the
// stream starts at the seek point, and for movies split over files), buffer, credits mark and a
// picture preview; volume, subtitles, audio, quality, speed, picture-in-picture, full screen,
// "Skip credits", the next episode with a countdown and keyboard shortcuts (? lists them).
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
const position = () => (P.base || 0) + P.offset + ((castOn() ? CAST.rp.currentTime : video.currentTime) || 0);
// A converted stream also "ends" when the server stops sending (ffmpeg failed, the conversion was stopped):
// only within a minute of the title's length (from the server, dur) is that the real end.
const reallyEnded = (pos, dur) => !dur || dur - pos <= 60;
// What the bar shows: a seek waiting to happen, else where the video is.
const shown = () => P.pending !== null ? P.pending : position();

// together = following a "Watch together" room (not told to the others again).
async function play(it, startAt, queue, together, small) {
  if (typeof stopTheme === 'function') stopTheme();
  P.queue = queue || [];
  if (!it.Media || !it.Media[0].streams) it = meta(await GET('/library/metadata/' + it.id))[0];
  P.it = it;
  if (!small) player.style.display = 'flex';
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
  if (it.type === 'movie' && !(startAt > 0) && !together && !castOn()) await playPreroll(it);
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
      if (finished) return; finished = true; P.preroll = false;
      video.removeEventListener('ended', end); video.removeEventListener('error', end);
      skip.remove(); if (next) next.remove(); done(!all && P.it === it && player.style.display !== 'none');
    };
    if (next) { next.onclick = end; player.append(next); }
    skip.onclick = () => { all = true; end(); };
    player.append(skip);
    video.addEventListener('ended', end); video.addEventListener('error', end);
    video.querySelectorAll('track').forEach(tr => tr.remove());
    P.preroll = true; // the film's own ended / error handlers leave this video alone
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
  // Watching in the small window only: the next one plays there too.
  const small = !!document.pictureInPictureElement && player.style.display === 'none';
  if (next) play(next, 0, P.queue, false, small); else closePlayer(false);
}
// Jump to a place in the whole video.
async function seekTo(s) {
  s = Math.max(0, Math.min(s, Math.max(0, duration() - 0.5)));
  P.pending = null;
  if (P.method === 'direct' && !split()) { if (castOn()) castSeek(s); else video.currentTime = s; } else await load(s);
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
// Night mode (⚙): a compressor evens out the sound, so explosions don't boom and voices stay clear;
// it works on any video (also played directly) and changes at once. Both go through one sound graph,
// made the first time one is needed (from then on the video's sound always runs through it).
const LEVEL = { ctx: null, gain: null, comp: null, night: store.get('nightMode', '0') === '1' };
function soundGraph() {
  if (LEVEL.gain) return;
  const ctx = LEVEL.ctx = new (window.AudioContext || window.webkitAudioContext)();
  LEVEL.gain = ctx.createGain();
  ctx.createMediaElementSource(video).connect(LEVEL.gain);
  // Loud parts down (about 6:1 above -32 dB) and a limiter at the end. The browser's compressor lifts its output by
  // itself, so it is brought down by half again: measured with a tone, -33 / -21 / -3.5 dB come out at
  // -25 / -20 / -16.5 dB, like the app's mpv filter.
  const comp = LEVEL.comp = ctx.createDynamicsCompressor();
  comp.threshold.value = -32; comp.knee.value = 12; comp.ratio.value = 6; comp.attack.value = 0.005; comp.release.value = 0.3;
  const makeup = ctx.createGain();
  makeup.gain.value = 0.5;
  const limit = ctx.createDynamicsCompressor();
  limit.threshold.value = -2; limit.knee.value = 0; limit.ratio.value = 20; limit.attack.value = 0.001; limit.release.value = 0.1;
  comp.connect(makeup); makeup.connect(limit); limit.connect(ctx.destination);
}
function soundRoute() {
  LEVEL.gain.disconnect();
  LEVEL.gain.connect(LEVEL.night ? LEVEL.comp : LEVEL.ctx.destination);
  LEVEL.ctx.resume();
}
function levelVolume(it) {
  try {
    const on = store.get('levelVolume', '0') === '1' && typeof it.loudness === 'number';
    if (!on && !LEVEL.gain && !LEVEL.night) return;
    soundGraph();
    LEVEL.gain.gain.value = on ? Math.min(2, Math.max(0.3, Math.pow(10, (-23 - it.loudness) / 20))) : 1;
    soundRoute();
  } catch (e) { /* the video just plays at its own volume */ }
}
function setNight(on) {
  LEVEL.night = on;
  store.set('nightMode', on ? '1' : '0');
  try { soundGraph(); soundRoute(); } catch (e) { LEVEL.night = false; return toast(t('Night mode does not work in this browser.')); }
  toast(on ? t('Night mode on: loud parts are quieter, voices are clearer.') : t('Night mode off'));
}
async function load(at) {
  const it = P.it, seq = P.loadSeq = (P.loadSeq || 0) + 1;
  levelVolume(it);
  let q = P.quality;
  // Settings → Web → quality: a video that is not bigger than the chosen quality plays as it is.
  const want = Number((/^(\d+)p/.exec(q) || [])[1]) || 0, have = Number(it.Media && it.Media[0] && it.Media[0].height) || 0;
  if (want && have && have <= want && store.get('keepSmall', '1') === '1') q = 'original';
  // A Chromecast plays H.264 up to 1080p for sure (the codecs the browser plays don't count there).
  const cast = castOn();
  if (cast && q === 'original' && have > 1080) q = '1080p-20';
  // Which part the position is in; `at` becomes the time inside that part.
  P.part = 0;
  if (split()) while (P.part < P.parts.length - 1 && at >= partStart(P.part + 1)) P.part++;
  P.base = split() ? partStart(P.part) : 0;
  at = Math.max(0, at - P.base);
  const extra = (CLIENT_CODECS && !cast ? `&videoCodecs=${CLIENT_CODECS}` : '') + (P.audio ? `&audio=${P.audio}` : '') + (P.part ? `&part=${P.part}` : '') + (P.burn !== null && P.burn !== undefined ? `&subtitle=${P.burn}` : '');
  const d = await GET(`/library/metadata/${it.id}/decision?quality=${q}${extra}`);
  P.tk = await ticket('item=' + it.id);
  if (P.it !== it || seq !== P.loadSeq) return; // a newer load (another quick seek) wins
  P.method = P.audio && d.method === 'direct' ? 'remux' : d.method;
  if (!d.allowed) toast(t('Converting is switched off for your account, so this file may not play here.'), 5000);
  if (cast && castOn()) {
    if (store.get('directPlay', '1') === '0' && P.method === 'direct' && d.allowed) P.method = 'transcode';
    await castLoad(it, at, q, extra);
    clearInterval(P.timer);
    P.timer = setInterval(() => { if (!isPaused() && CAST.media) report('playing'); }, 10000);
    drawBar();
    return;
  }
  video.querySelectorAll('track').forEach(tr => tr.remove());
  spin(true);
  const noDirect = store.get('directPlay', '1') === '0' && P.method === 'direct' && d.allowed;
  if (noDirect) P.method = 'transcode';
  if (P.method === 'direct') {
    P.offset = 0;
    video.src = withTicket(`/library/parts/${it.id}/file${P.part ? '?part=' + P.part : ''}`, P.tk);
    video.addEventListener('loadedmetadata', () => { video.currentTime = at; }, { once: true });
  } else {
    P.offset = at;
    video.src = withTicket(`/video/transcode/${it.id}/stream.mp4?quality=${q}&offset=${at.toFixed(1)}${extra}${noDirect ? '&transcode=1' : ''}`, P.tk);
  }
  addSubtitles();
  $('#pmethod').textContent = { direct: ' · ' + t('direct play'), remux: ' · ' + t('repackaged'), transcode: ' · ' + t('converting') }[P.method] || '';
  // A new source starts at the default rate: keep the chosen speed.
  video.defaultPlaybackRate = video.playbackRate = P.speed;
  // (The browser may refuse to start by itself; then the play button is there.)
  video.play().catch(() => { spin(false); drawButtons(); });
  clearInterval(P.timer);
  P.timer = setInterval(() => { if (!isPaused()) report('playing'); }, 10000);
  drawBar();
}
// Subtitles belong to the first file (a later part would show them at the wrong time). A converted
// stream starts at the seek point, so its times are moved by that much.
function subTrack(s, i, shift, fresh) {
  const el = h('track', { kind: 'subtitles', src: withTicket(s.key + (fresh ? '?r=' + Date.now() : ''), P.tk), label: subName(s, i), srclang: (s.language || 'und').slice(0, 2) });
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
  // The burned-in picture has to go; a TV gets the media again with the new subtitle.
  if (before !== null || castOn()) load(position());
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
  const ends = d && !isPaused() ? new Date(Date.now() + (d - now) / (castOn() ? 1 : video.playbackRate || 1) * 1000) : null;
  const sleep = !P.sleep ? '' : P.sleep.end ? t('stops after this title') : t('sleep timer {time}', { time: fmtTime(sleepLeft()) });
  $('#psub').textContent = [P.subLine, ends ? t('ends at {time}', { time: ends.toLocaleTimeString(I18N.lang, { hour: '2-digit', minute: '2-digit', hour12: hour12() }) }) : '', sleep].filter(Boolean).join(' · ');
  // "Skip credits" while the credits run.
  const inCredits = P.credits.some(c => now >= c) && now < d - 3;
  const skip = $('#pskip');
  if (inCredits && $('#pnext').style.display !== 'flex') {
    const next = P.queue[0] || Q.list[0] || P.next;
    skip.textContent = !next ? t('Skip credits') : next.type === 'episode' ? t('Next episode ▸') : t('Play next ▸');
    showSkip(skip, true);
  } else showSkip(skip, false);
  // "Skip intro" while the intro runs.
  const si = $('#pskipintro');
  if (P.intro && now >= P.intro[0] && now < P.intro[1] - 1) { si.textContent = t('Skip intro'); showSkip(si, true); }
  else showSkip(si, false);
}
// A skip button that just came up stays SKIP_FRESH_MS even when the controls have faded out,
// then goes with them (moving the mouse brings it back while the intro or credits still run).
const SKIP_FRESH_MS = 10000;
function showSkip(el, on) {
  const was = el.style.display === 'block';
  el.style.display = on ? 'block' : 'none';
  if (on && !was) {
    el.classList.add('fresh');
    clearTimeout(el.freshTimer);
    el.freshTimer = setTimeout(() => el.classList.remove('fresh'), SKIP_FRESH_MS);
  }
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
  const pb = $('#pplay'), playing = !isPaused();
  pb.replaceChildren(icon(playing ? 'pause' : 'play'));
  pb.title = (playing ? t('Pause') : t('Play')) + ' (K)';
  // While casting: the TV's volume.
  const muted = castOn() ? CAST.rp.isMuted : video.muted, volume = castOn() ? CAST.rp.volumeLevel : video.volume;
  $('#pmute').replaceChildren(icon(muted || volume === 0 ? 'mute' : volume < 0.5 ? 'low' : 'vol'));
  $('#pmute').title = (muted ? t('Sound on') : t('Mute')) + ' (M)';
  $('#pvolume').value = muted ? 0 : volume;
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
  idleTimer = setTimeout(() => { if (!isPaused() && !P.menu && !bar.classList.contains('drag') && $('#pkeys').style.display !== 'block') player.classList.add('idle'); }, 3000);
}
player.addEventListener('mousemove', wake);
player.addEventListener('touchstart', wake, { passive: true });

// ---- video events
// A context started without a user gesture stays suspended (Safari), so resume it inside the click.
video.addEventListener('play', () => { if (LEVEL.ctx && LEVEL.ctx.state === 'suspended') LEVEL.ctx.resume(); drawButtons(); wake(); });
// A new source (another quality, or a jump in a converted video) pauses the old one
// on its way: that is no pause (readyState is 0 then). Playing again is told at once.
video.addEventListener('pause', () => { if (castOn()) return; drawButtons(); wake(); if (!video.ended && video.readyState > 0) report('paused'); });
video.addEventListener('playing', () => { if (P.reported !== 'playing') report('playing'); });
video.addEventListener('volumechange', () => { drawButtons(); if (P.sleepVol !== undefined) return; store.set('volume', video.volume); store.set('muted', video.muted ? '1' : '0'); });
video.addEventListener('timeupdate', () => { if (!bar.classList.contains('drag')) drawBar(); drawLyrics(); });
video.addEventListener('progress', drawBar);
video.addEventListener('durationchange', drawMarks);
video.addEventListener('waiting', () => spin(true));
video.addEventListener('seeking', () => spin(true));
['playing', 'canplay', 'loadeddata', 'seeked', 'pause', 'error'].forEach(ev => video.addEventListener(ev, () => spin(false)));
// A button clicked with the mouse lets go of the focus, so Space keeps meaning play / pause.
player.addEventListener('pointerup', e => { const b = e.target.closest('.pbtn'); if (b && e.pointerType === 'mouse') setTimeout(() => b.blur(), 0); });
video.addEventListener('error', () => {
  if (P.preroll || !P.it || !video.getAttribute('src')) return;
  // The address's own ticket has (nearly) run out, after hours of playing or a long pause: go on from here with a new one.
  // (Not the cache: hovering the seek bar may have renewed that while the video kept the old one.)
  if (ticketEnd(P.tk) - Date.now() / 1000 < 600) { load(position()); return; }
  // Stopped by the owner? Then the report says so (and shows why).
  report('playing').then(r => { if (!(r && r.stopped)) toast(t('This video could not be played. Try a lower quality in ⚙.'), 6000); });
});
video.addEventListener('ended', () => { if (!castOn()) videoEnded(); });
// The end of the video, in the browser or on the TV.
function videoEnded() {
  if (P.preroll || !P.it) return;
  // The next part of a split movie.
  if (split() && P.part < P.parts.length - 1) { load(partStart(P.part + 1)); return; }
  // (A finished Chromecast no longer says where it was.)
  if (!castOn() && !reallyEnded(position(), P.it && P.it.duration / 1000)) {
    report('paused').then(r => { if (!(r && r.stopped)) toast(t('The video stopped before its end. Your place was saved.'), 6000); });
    return;
  }
  report('stopped', Math.floor(duration() * 1000));
  upNext();
}
// The end, or "Skip credits": the next one after a countdown, or close.
function upNext() {
  if (sleepAtEnd()) return;
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

// ---- sleep timer: pauses after a number of minutes (clock time, the sound fading out over the
// last 20 s), or stops at the end of this episode or movie (at its credits when they are known,
// which then counts it as watched). It lasts over episodes until it fires or the player closes.
// Any input while the sound fades switches it off again.
const SLEEP_MINUTES = [15, 30, 45, 60, 90, 120], SLEEP_FADE = 20;
function setSleep(v) {
  sleepFade(1);
  P.sleep = v === null ? null : v === 'end' ? { end: true } : { at: Date.now() + v * 60000 };
  drawBar();
}
function sleepLeft() { return P.sleep && P.sleep.at ? Math.max(0, (P.sleep.at - Date.now()) / 1000) : null; }
function sleepLabel() {
  if (!P.sleep) return t('Off');
  if (P.sleep.end) return t('End of this title');
  return t('{n} min left', { n: Math.ceil(sleepLeft() / 60) });
}
// The fade turns the volume down without keeping it as the chosen volume; 1 puts it back.
function sleepFade(f) {
  if (f >= 1) { if (P.sleepVol === undefined) return; const v = P.sleepVol; delete P.sleepVol; video.volume = v; return; }
  if (P.sleepVol === undefined) P.sleepVol = video.volume;
  video.volume = P.sleepVol * Math.max(0, f);
}
function sleepNow() {
  P.sleep = null;
  if (castOn()) { if (!isPaused()) castTogglePlay(); } else video.pause();
  sleepFade(1);
  hideNext(); closePlayerMenu();
  drawBar();
  toast(t('Sleep timer: paused. Good night!'), 8000);
}
// At the end of the title, instead of the next one: stop (the end was already reported).
function sleepAtEnd() {
  if (!P.sleep || !P.sleep.end) return false;
  P.sleep = null;
  closePlayer(false);
  toast(t('Sleep timer: stopped after this title. Good night!'), 8000);
  return true;
}
setInterval(() => {
  if (!P.it || !P.sleep || P.preroll) return;
  if (P.sleep.end) {
    // The credits count as the end, like "Skip credits".
    const now = position(), d = duration();
    if (P.credits.length && P.credits.some(c => now >= c) && now < d - 3 && !isPaused()) {
      report('stopped', Math.floor(d * 1000));
      sleepAtEnd();
    }
    return;
  }
  const left = sleepLeft();
  if (left <= 0) return sleepNow();
  if (left <= SLEEP_FADE && !video.paused) sleepFade(left / SLEEP_FADE);
  if (player.style.display === 'flex') drawBar();
}, 1000);
for (const ev of ['pointerdown', 'keydown', 'touchstart', 'wheel']) document.addEventListener(ev, () => {
  if (P.sleepVol !== undefined && P.sleep) { setSleep(null); toast(t('Sleep timer off')); }
}, { passive: true, capture: true });
$('#pskipintro').addEventListener('click', () => {
  $('#pskipintro').style.display = 'none';
  if (P.intro) seekTo(P.intro[1]).then(() => tgSend());
});
$('#pskip').addEventListener('click', () => {
  $('#pskip').style.display = 'none';
  // Counts as watched to the end, like letting the credits run.
  report('stopped', Math.floor(duration() * 1000));
  if (sleepAtEnd()) return;
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
  frameTimer = setTimeout(async () => {
    if (!P.it || !P.it.Media) return;
    const src = withTicket(`/library/parts/${P.it.id}/frame?t=${step}`, await ticket('item=' + P.it.id));
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
const togglePlay = () => { if (castOn()) castTogglePlay(); else if (video.paused) { video.play().catch(() => {}); flash('play'); } else { video.pause(); flash('pause'); } };
$('#pplay').addEventListener('click', togglePlay);
$('#pback').addEventListener('click', () => nudge(-10));
$('#pfwd').addEventListener('click', () => nudge(30));
$('#pnextbtn').addEventListener('click', () => { report('stopped'); playNext(); });
$('#pmute').addEventListener('click', () => { if (castOn()) { CAST.rpc.muteOrUnmute(); return; } video.muted = !video.muted; if (!video.muted && video.volume === 0) video.volume = 0.5; });
// The last and first bit of the track count as full and silent (the thumb's travel is shorter than the track).
// Dragging works from the pointer's position on the track, also past either end (so 100 % and 0 % are easy to hit),
// and keeps the controls awake meanwhile.
{
  const vol = $('#pvolume');
  const setFrom = e => {
    const r = vol.getBoundingClientRect(), pad = 8; // the thumb's half width
    let v = Math.max(0, Math.min(1, (e.clientX - r.left - pad) / Math.max(1, r.width - 2 * pad)));
    if (v > 0.96) v = 1; if (v < 0.03) v = 0;
    vol.value = v;
    if (castOn()) { castVolume(v); return; }
    video.volume = v; video.muted = v === 0;
  };
  vol.addEventListener('pointerdown', e => { if (e.button) return; e.preventDefault(); vol.focus(); vol.setPointerCapture(e.pointerId); vol.dataset.drag = '1'; setFrom(e); wake(); });
  vol.addEventListener('pointermove', e => { if (vol.dataset.drag) { setFrom(e); wake(); } });
  const done = e => { delete vol.dataset.drag; wake(); };
  vol.addEventListener('pointerup', done); vol.addEventListener('pointercancel', done);
}
$('#pvolume').addEventListener('input', e => { let v = Number(e.target.value); if (v > 0.96) v = 1; if (v < 0.03) v = 0; if (castOn()) return castVolume(v); video.volume = v; video.muted = v === 0; });
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
      P.it && P.it.type !== 'track' ? choice(t('Night mode'), LEVEL.night, () => { setNight(!LEVEL.night); playerMenu('settings', 'main'); }) : null,
      entry(t('Sleep timer'), sleepLabel(), () => playerMenu('settings', 'sleep')),
      h('div', { class: 'mi', tabindex: 0, onclick: e => { e.stopPropagation(); closePlayerMenu(); showKeys(); } }, h('span', { class: 'ck' }), t('Keyboard shortcuts'), h('span', { class: 'v' }, '?'))];
  } else if (page === 'quality') {
    kids = [back(t('Quality')), ...QUALITIES.map(([v, l]) => choice(l, P.quality === v, () => { P.quality = v; store.set('quality', v); closePlayerMenu(); load(position()); }))];
  } else if (page === 'speed') {
    kids = [back(t('Playback speed')), ...SPEEDS.map(v => choice(v === 1 ? t('Normal speed') : fmtNum(v) + '×', P.speed === v, () => { setSpeed(v); playerMenu('settings', 'speed'); }))];
  } else if (page === 'sleep') {
    const pick = v => { setSleep(v); closePlayerMenu(); if (v !== null) toast(t('Sleep timer: {when}', { when: sleepLabel() })); };
    kids = [back(t('Sleep timer')), choice(t('Off'), !P.sleep, () => pick(null)),
      choice(P.it && P.it.type === 'episode' ? t('End of this episode') : t('End of this movie'), !!(P.sleep && P.sleep.end), () => pick('end')),
      ...SLEEP_MINUTES.map(n => choice(t('{n} minutes', { n }), false, () => pick(n)))];
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
  castStop();
  tgLeave();
  clearInterval(P.timer); clearTimeout(P.pendingTimer); hideNext(); closePlayerMenu();
  if (document.fullscreenElement) document.exitFullscreen().catch(() => {});
  if (document.pictureInPictureElement) document.exitPictureInPicture().catch(() => {});
  video.pause(); video.removeAttribute('src'); video.querySelectorAll('track').forEach(tr => tr.remove()); video.load();
  P.lyrics = null; $('#plyrics').style.display = 'none'; $('#plyricsbtn').hidden = true;
  player.style.display = 'none';
  player.classList.remove('idle');
  P.it = null; P.next = null; P.pending = null; P.autoRun = 0; P.sleep = null; sleepFade(1);
  if ('mediaSession' in navigator) navigator.mediaSession.metadata = null;
  render();
}
// With picture in picture on, "back" only puts the big player away: the video plays on in its
// small window while one looks around. Closing that window (✕, which pauses) ends playback;
// "back to tab" brings the big player back.
function leavePlayer() {
  if (!document.pictureInPictureElement) return closePlayer(true);
  closePlayerMenu(); hideNext();
  if (document.fullscreenElement) document.exitFullscreen().catch(() => {});
  player.style.display = 'none';
  render();
}
video.addEventListener('leavepictureinpicture', () => {
  if (!P.it || player.style.display !== 'none') return;
  // The browser pauses the video just after this when ✕ was clicked.
  setTimeout(() => {
    if (!P.it || player.style.display !== 'none') return;
    if (video.paused) closePlayer(true); else player.style.display = 'flex';
  }, 300);
});
$('#pclose').addEventListener('click', leavePlayer);

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
    return leavePlayer();
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
