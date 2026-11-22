<script setup lang="ts">
// The player: the whole window, own controls over a <video>. The server decides how a file plays
// (GET …/decision): as it is (the file itself, the browser seeks in it), or repackaged / converted as a
// stream that starts at the seek point (stream.mp4?offset=…), so one seek bar covers the whole video either way.
// Progress goes to the server every 10 s and on pause and leaving (/:/timeline). An episode goes on with the next.
// A movie split over files plays its parts one after the other, still with one seek bar.
// ⚙ has the quality, speed, night mode and whether the next episode plays by itself; chapters, intro, recap
// and credits are marked on the seek bar, with buttons to skip the intro / recap and the credits. At the end
// (or on "Skip credits") the next episode comes after a countdown; after a few in a row without anyone
// touching anything, the player asks whether somebody is still watching.
import { computed, onBeforeUnmount, onMounted, ref, watch } from 'vue';
import { useRoute, useRouter } from 'vue-router';
import { api, get, list, picture, pref, savePref, ticket, withTicket, type Container, type Item, type Stream, type Subtitle } from '../api';
import { fmtClock, fmtNum, fmtTime, langName, lang2, resumeAt } from '../format';
import { t } from '../i18n';
import Icon from '../components/Icon.vue';
import { nextInQueue, queue, watching } from '../queue';

const props = defineProps<{ id: string }>();
const route = useRoute();
const router = useRouter();
const video = ref<HTMLVideoElement | null>(null);
const box = ref<HTMLElement | null>(null);
const it = ref<Item | null>(null);
const error = ref('');
const method = ref<'direct' | 'remux' | 'transcode'>('direct');
const offset = ref(0); // where a converted stream starts in the video (s)
const now = ref(0); // the position in the whole video (s)
const buffered = ref(0);
const paused = ref(true);
const waiting = ref(true);
const muted = ref(false);
const volume = ref(1);
const full = ref(false);
const idle = ref(false);
const menu = ref<'' | 'subs' | 'audio' | 'settings' | 'chapters'>('');
const page = ref<'main' | 'quality' | 'speed'>('main'); // the page of ⚙ that shows
const audio = ref<number | null>(null); // the audio stream's index, null = the file's first
const sub = ref(-1); // the text subtitle shown (index in subs), -1 = none
const burn = ref<number | null>(null); // the picture subtitle burned into the video (index in subs)
const part = ref(0); // the file of a split movie that plays
const base = ref(0); // where that file starts in the whole movie (s)
const stopped = ref(false); // the owner stopped this playback (from the dashboard)
const stoppedMsg = ref('');
const upNext = ref<Item | null>(null); // the next episode (looked up when the video starts)
let tk = '';
let timer = 0;
let idleTimer = 0;
let loadSeq = 0;

const PICTURE_SUBS = ['pgs', 'dvd_sub', 'dvb_sub', 'xsub', 'vobsub', 'hdmv'];
const media = computed(() => (it.value && it.value.Media && it.value.Media[0]) || null);
const duration = computed(() => ((it.value && it.value.duration) || 0) / 1000 || (video.value && isFinite(video.value.duration) ? video.value.duration + offset.value : 0));
const audios = computed<Stream[]>(() => ((media.value && media.value.streams) || []).filter(s => s.kind === 'audio'));
const subs = computed<Subtitle[]>(() => (media.value && media.value.subtitles) || []);
// Subtitles made of pictures (Blu-ray, DVD) can't be shown as text: the server burns them into the video.
const isPicture = (s?: Subtitle) => !!s && !!s.embedded && PICTURE_SUBS.some(k => String(s.codec || '').toLowerCase().includes(k));
// A movie split over files: how long each part is (s). Only when every length is known.
const parts = computed(() => ((media.value && media.value.Part) || []).map(x => (x.duration || 0) / 1000));
const split = computed(() => parts.value.length > 1 && parts.value.every(d => d > 0));
const partStart = (n: number) => parts.value.slice(0, n).reduce((a, b) => a + b, 0);
const title = computed(() => (it.value ? (it.value.type === 'episode' ? it.value.grandparentTitle : it.value.title) : ''));
const subLine = computed(() => {
  const i = it.value; if (!i) return '';
  return i.type === 'episode' ? `${t('S{n}', { n: i.parentIndex || 0 })}${t('E{n}', { n: i.index || 0 })} · ${i.title}` : i.year ? String(i.year) : '';
});
const subName = (s: Subtitle, i: number) => [s.language ? langName(s.language) : '', s.title, s.forced ? t('forced') : '', s.sdh ? 'SDH' : '', !s.language && s.file ? s.file : ''].filter(Boolean).join(' · ') || t('Subtitles {n}', { n: i + 1 });
const audioName = (a: Stream) => [a.language ? langName(a.language) : '', String(a.codec || '').toUpperCase(), a.channels === 6 ? '5.1' : a.channels === 8 ? '7.1' : a.channels === 2 ? t('Stereo') : '', a.title || ''].filter(Boolean).join(' · ');

const CODECS = (() => {
  const v = document.createElement('video'), ok = (type: string) => { try { return !!v.canPlayType(type); } catch { return false; } };
  return [ok('video/mp4; codecs="hvc1.1.6.L153.B0"') ? 'hevc' : '', ok('video/webm; codecs="vp9"') ? 'vp9' : '', ok('video/mp4; codecs="av01.0.08M.08"') ? 'av1' : ''].filter(Boolean).join(',');
})();

/** Loads the video at `at` seconds (a converted stream: asks for a new one starting there). */
async function load(at: number) {
  const i = it.value, v = video.value;
  if (!i || !v) return;
  const seq = ++loadSeq;
  waiting.value = true;
  // Settings of the account (Settings → Web in the classic app): quality, and a video not bigger than it plays as it is.
  let q = pref('quality', 'original');
  const want = Number((/^(\d+)p/.exec(q) || [])[1]) || 0, have = Number(media.value && media.value.height) || 0;
  if (want && have && have <= want && pref('keepSmall', '1') !== '0') q = 'original';
  // Which part of a split movie the position is in; `at` becomes the time inside that part.
  let p = 0;
  if (split.value) while (p < parts.value.length - 1 && at >= partStart(p + 1)) p++;
  part.value = p; base.value = split.value ? partStart(p) : 0;
  at = Math.max(0, at - base.value);
  const extra = (CODECS ? `&videoCodecs=${CODECS}` : '') + (audio.value !== null ? `&audio=${audio.value}` : '') + (p ? `&part=${p}` : '')
    + (burn.value !== null ? `&subtitle=${burn.value}` : '');
  const d = await get<{ method: 'direct' | 'remux' | 'transcode', allowed: boolean }>(`/library/metadata/${i.id}/decision?quality=${q}${extra}`);
  tk = await ticket('item=' + i.id);
  if (seq !== loadSeq) return;
  let m = audio.value !== null && d.method === 'direct' ? 'remux' : d.method;
  const noDirect = pref('directPlay', '1') === '0' && m === 'direct' && d.allowed;
  if (noDirect) m = 'transcode';
  method.value = m;
  levelVolume();
  if (m === 'direct') {
    offset.value = 0;
    v.src = withTicket(`/library/parts/${i.id}/file${p ? '?part=' + p : ''}`, tk);
    v.addEventListener('loadedmetadata', () => { v.currentTime = at; }, { once: true });
  } else {
    offset.value = at;
    v.src = withTicket(`/video/transcode/${i.id}/stream.mp4?quality=${q}&offset=${at.toFixed(1)}${extra}${noDirect ? '&transcode=1' : ''}`, tk);
  }
  v.defaultPlaybackRate = v.playbackRate = speed.value;
  addSubtitles();
  v.play().catch(() => { waiting.value = false; paused.value = true; });
}

// Text subtitles as <track>s; a converted stream starts at the seek point, so their times move by as much.
// They belong to the first file of a split movie (a later part would show them at the wrong time).
let tracks: number[] = []; // the subtitle (index in subs) of each <track>
function addSubtitles() {
  const v = video.value; if (!v) return;
  v.querySelectorAll('track').forEach(tr => tr.remove());
  tracks = [];
  if (part.value) return;
  subs.value.forEach((s, i) => {
    if (isPicture(s)) return;
    tracks.push(i);
    const el = document.createElement('track');
    el.kind = 'subtitles'; el.label = subName(s, i); el.srclang = (s.language || 'und').slice(0, 2);
    el.src = withTicket(s.key, tk);
    const shift = offset.value;
    el.addEventListener('load', () => {
      if (shift > 0 && el.track.cues) for (const c of Array.from(el.track.cues)) { c.startTime = Math.max(0, c.startTime - shift); c.endTime = Math.max(0, c.endTime - shift); }
    }, { once: true });
    v.append(el);
  });
  showSub();
}
function showSub() {
  const v = video.value; if (!v) return;
  Array.from(v.textTracks).forEach((tr, n) => {
    tr.mode = tracks[n] === sub.value ? 'showing' : 'disabled';
    tr.oncuechange = () => placeCues();
  });
  placeCues();
}
// Where the lines sit: Settings → Subtitle position lifts them above the bottom, and while the controls
// are shown they move up above those, so they are not hidden behind the seek bar.
function placeCues() {
  const v = video.value; if (!v) return;
  const up = Number(pref('subLift', '0')) || 0;
  const bar = !idle.value && bottom.value ? bottom.value.offsetHeight : 0;
  const h = v.clientHeight || 1;
  for (const tr of Array.from(v.textTracks)) {
    if (tr.mode !== 'showing') continue;
    for (const c of Array.from(tr.activeCues || []) as VTTCue[]) {
      if (bar && bar / h < 0.5) { c.snapToLines = false; c.lineAlign = 'end'; c.line = Math.round((1 - bar / h) * 100) - 2 - up * 5; }
      else if (up) { c.snapToLines = true; c.lineAlign = 'start'; c.line = -(1 + up); }
      else { c.snapToLines = true; c.lineAlign = 'start'; c.line = 'auto'; }
    }
  }
}

// The audio and subtitles a video starts with (Settings → Playback), as in the classic app: the preferred
// audio language; the subtitle language picked last time, else the subtitle mode decides (always, or with
// foreign audio) and forced subtitles come on with audio in the preferred language.
function pickSub(lang: string, sdh: string, forced: boolean) {
  const all = subs.value.map((s, i) => ({ s, i })).filter(x => lang2(x.s.language) === lang && (!isPicture(x.s) || burnOk()));
  let pool = forced ? all.filter(x => x.s.forced) : all.filter(x => !x.s.forced);
  if (!forced && !pool.length) pool = all;
  if (!pool.length) return -1;
  const rank = (x: { s: Subtitle }) => (sdh === 'prefer' ? (x.s.sdh ? 0 : 1) : sdh === 'avoid' ? (x.s.sdh ? 1 : 0) : 0);
  return pool.map(x => [rank(x), x.i]).sort((p, q) => p[0] - q[0] || p[1] - q[1])[0][1];
}
function chooseTracks() {
  const prefAudio = pref('audioLang');
  const at = prefAudio ? audios.value.findIndex(a => lang2(a.language) === prefAudio) : -1;
  if (at > 0) audio.value = audios.value[at].index ?? null;
  const last = pref('subLang');
  sub.value = last ? subs.value.findIndex(s => (s.language || '') === last && !s.forced) : -1;
  const mode = pref('subMode', 'manual'), want = pref('subPrefLang');
  if (sub.value < 0 && want) {
    const playing = audios.value[at > 0 ? at : 0];
    const foreign = !playing || !playing.language || lang2(playing.language) !== want;
    if (mode !== 'manual' && (mode === 'always' || foreign)) sub.value = pickSub(want, pref('subSdh', 'any'), false);
    else if (!foreign && pref('subForced', '1') === '1') sub.value = pickSub(want, 'any', true);
  }
  // A picture subtitle is burned in (when Settings → Playback allows it).
  if (sub.value >= 0 && isPicture(subs.value[sub.value])) { if (burnOk()) burn.value = sub.value; sub.value = -1; }
}
const burnOk = () => pref('burnSubs', 'auto') === 'auto';
// Size, colour and background of the subtitles, as CSS variables for video::cue.
const cueStyle = computed(() => ({
  '--cue': pref('subSize', '100') + '%',
  '--cue-color': pref('subColor', '#ffffff'),
  '--cue-bg': pref('subBg', '1') === '1' ? 'rgba(0,0,0,.55)' : 'transparent',
}));
const SUB_SIZES = [['75', 'Small'], ['100', 'Normal'], ['130', 'Large'], ['170', 'Very large']];
function setSubSize(v: string) { savePref('subSize', v).catch(() => {}); }
const notice = ref('');
let noticeTimer = 0;
function say(text: string) { notice.value = text; clearTimeout(noticeTimer); noticeTimer = window.setTimeout(() => { notice.value = ''; }, 5000); }
function setSub(i: number) {
  menu.value = '';
  const before = burn.value, s = subs.value[i];
  burn.value = null;
  if (isPicture(s)) {
    if (!burnOk()) { say(t('This subtitle is made of pictures; turn on burning in under Settings → Playback.')); i = -1; }
    else { burn.value = i; i = -1; }
  }
  sub.value = i;
  showSub();
  savePref('subLang', s && (i >= 0 || burn.value !== null) ? s.language || '' : '').catch(() => {});
  // A burned-in picture comes or goes: the video is fetched again from here.
  if (before !== burn.value) load(now.value);
}
function setAudio(a: Stream, i: number) {
  menu.value = '';
  audio.value = i === 0 ? null : (a.index ?? null);
  load(now.value);
}

// Quality (⚙): kept with the account (Settings → Web in the classic app); the video loads again from here.
const QUALITIES = [['original', 'Original quality'], ['1080p-20', '1080p 20 Mbps'], ['1080p-12', '1080p 12 Mbps'], ['1080p-8', '1080p 8 Mbps'],
  ['720p-4', '720p 4 Mbps'], ['720p-3', '720p 3 Mbps'], ['720p-2', '720p 2 Mbps'], ['480p-1.5', '480p 1.5 Mbps'], ['320p-0.7', '320p 0.7 Mbps']];
const quality = computed(() => pref('quality', 'original'));
const qualityName = (v: string) => { const q = QUALITIES.find(x => x[0] === v); return !q ? v : v === 'original' ? t(q[1]) : q[1].replace(/\d+\.\d+/, n => fmtNum(Number(n))); };
function setQuality(v: string) {
  menu.value = '';
  if (v === quality.value) return;
  savePref('quality', v).catch(() => {});
  load(now.value);
}
// Whether the next episode (or what comes next in a playlist) plays by itself after a countdown.
const autoNext = computed(() => pref('autoNext', '1') !== '0');
const hasNext = computed(() => !!it.value && (it.value.type === 'episode' || !!route.query.queue));

// Speed (⚙, or < and >): kept with the account, like in the classic app.
const SPEEDS = [0.5, 0.75, 1, 1.25, 1.5, 1.75, 2];
const speed = ref(SPEEDS.includes(Number(pref('speed', '1'))) ? Number(pref('speed', '1')) : 1);
const speedName = (x: number) => (x === 1 ? t('Normal speed') : fmtNum(x) + '×');
function setSpeed(x: number, tell = false) {
  speed.value = x;
  const v = video.value; if (v) v.defaultPlaybackRate = v.playbackRate = x;
  savePref('speed', String(x)).catch(() => {});
  if (tell) say(t('Speed: {speed}', { speed: speedName(x) }));
}

// Night mode (⚙): a compressor evens out the sound, so explosions don't boom and voices stay clear. "Level the
// volume" (classic app settings) brings measured videos to about -23 LUFS. Both go through one sound graph,
// made the first time one is needed; the same filter values as in the classic app.
const night = ref(pref('nightMode', '0') === '1');
let sound: { ctx: AudioContext, gain: GainNode, comp: DynamicsCompressorNode } | null = null;
function soundGraph() {
  if (sound) return sound;
  const ctx = new AudioContext(), gain = ctx.createGain();
  ctx.createMediaElementSource(video.value!).connect(gain);
  const comp = ctx.createDynamicsCompressor();
  comp.threshold.value = -32; comp.knee.value = 12; comp.ratio.value = 6; comp.attack.value = 0.005; comp.release.value = 0.3;
  const makeup = ctx.createGain();
  makeup.gain.value = 0.5;
  const limit = ctx.createDynamicsCompressor();
  limit.threshold.value = -2; limit.knee.value = 0; limit.ratio.value = 20; limit.attack.value = 0.001; limit.release.value = 0.1;
  comp.connect(makeup); makeup.connect(limit); limit.connect(ctx.destination);
  return (sound = { ctx, gain, comp });
}
function soundRoute() {
  const g = soundGraph();
  g.gain.disconnect();
  g.gain.connect(night.value ? g.comp : g.ctx.destination);
  g.ctx.resume().catch(() => {});
}
function levelVolume() {
  const i = it.value; if (!i) return;
  const on = pref('levelVolume', '0') === '1' && typeof i.loudness === 'number';
  if (!on && !sound && !night.value) return;
  try {
    soundGraph().gain.gain.value = on ? Math.min(2, Math.max(0.3, Math.pow(10, (-23 - (i.loudness as number)) / 20))) : 1;
    soundRoute();
  } catch { /* the video just plays at its own volume */ }
}
function setNight(on: boolean) {
  menu.value = '';
  night.value = on;
  savePref('nightMode', on ? '1' : '0').catch(() => {});
  try { soundRoute(); } catch { night.value = false; say(t('Night mode does not work in this browser.')); return; }
  say(on ? t('Night mode on: loud parts are quieter, voices are clearer.') : t('Night mode off'));
}

// Chapters of the file, and the intro, recap and credits the server knows (s, in the whole video).
const chapters = ref<{ at: number, title: string }[]>([]);
const span = (m?: { start: number, end: number }) => (m && m.end > m.start ? [m.start / 1000, m.end / 1000] as [number, number] : null);
const intro = computed(() => span(it.value?.intro));
const recap = computed(() => span(it.value?.recap));
const credits = computed(() => (it.value?.credits || []).map(ms => ms / 1000));
const at = (s: number) => (duration.value ? Math.min(100, (s / duration.value) * 100) : 0);
const chapterAt = (s: number) => (chapters.value.length > 1 ? [...chapters.value].reverse().find(c => c.at <= s) || null : null);
// "Skip intro" / "Skip recap" while they run; "Skip credits" (or on to what comes next) while the credits do.
const skipBtn = computed<{ label: string, to: number } | null>(() => {
  const n = now.value, d = duration.value;
  for (const [m, label] of [[intro.value, 'Skip intro'], [recap.value, 'Skip recap']] as const) if (m && n >= m[0] && n < m[1] - 1) return { label: t(label), to: m[1] };
  if (d && credits.value.some(c => n >= c) && n < d - 3) {
    const nxt = route.query.queue ? queue.items[0] : upNext.value;
    return { label: !nxt ? t('Skip credits') : nxt.type === 'episode' ? t('Next episode ▸') : t('Play next ▸'), to: -1 };
  }
  return null;
});
// A skip button that just came up stays 10 s even when the controls fade out, then goes with them.
const skipFresh = ref(false);
let freshTimer = 0;
watch(() => skipBtn.value && skipBtn.value.label, (l, was) => {
  if (!l || l === was) return;
  skipFresh.value = true; clearTimeout(freshTimer);
  freshTimer = window.setTimeout(() => { skipFresh.value = false; }, 10000);
});
function doSkip() {
  const b = skipBtn.value; if (!b) return;
  if (b.to >= 0) seek(b.to); else finish();
}
// The time and chapter under the pointer on the seek bar.
const tip = ref<{ x: number, text: string } | null>(null);
function hover(e: MouseEvent) {
  const r = (e.currentTarget as HTMLElement).getBoundingClientRect();
  const f = Math.max(0, Math.min(1, (e.clientX - r.left) / r.width)), s = f * duration.value, c = chapterAt(s);
  tip.value = { x: Math.max(40, Math.min(r.width - 40, e.clientX - r.left)), text: fmtTime(s) + (c ? ' · ' + c.title : '') };
}

function seek(to: number) {
  const v = video.value; if (!v) return;
  to = Math.max(0, Math.min(duration.value ? duration.value - 1 : to, to));
  now.value = to;
  // Another part of a split movie: that file is loaded.
  const inPart = !split.value || (to >= base.value && (part.value === parts.value.length - 1 || to < partStart(part.value + 1)));
  if (!inPart) { load(to); return; }
  if (method.value === 'direct') { v.currentTime = to - base.value; return; }
  // Converted: within what has arrived already the stream itself seeks, else a new stream from there.
  const local = to - base.value - offset.value;
  if (local >= 0 && local <= buffered.value) v.currentTime = local;
  else load(to);
}
const skip = (s: number) => seek(now.value + s);
function toggle() {
  const v = video.value; if (!v) return;
  if (v.paused) v.play().catch(() => {}); else v.pause();
}
function setVolume(x: number) {
  const v = video.value; if (!v) return;
  v.volume = x; v.muted = x === 0;
  try { localStorage.setItem('volume', String(x)); } catch { /* none */ }
}
function toggleMute() { const v = video.value; if (v) v.muted = !v.muted; }
function toggleFull() {
  if (document.fullscreenElement) document.exitFullscreen().catch(() => {});
  else if (box.value) box.value.requestFullscreen().catch(() => {});
}

function report(state: 'playing' | 'paused' | 'stopped', ms = Math.floor(now.value * 1000)) {
  const i = it.value; if (!i) return Promise.resolve();
  return api<{ stopped?: boolean, message?: string } | null>('POST', `/:/timeline?ratingKey=${i.id}&time=${ms}&state=${state}&client=web`)
    .then(r => {
      // The owner stopped this playback from the dashboard.
      if (r && r.stopped) { stopped.value = true; stoppedMsg.value = r.message || ''; video.value && video.value.pause(); clearInterval(timer); }
    }).catch(() => {});
}

// Not started from a playlist: what was left of one is forgotten.
if (!route.query.queue) queue.items = [];

// After an episode: the next one in its season, then the first of the next season.
async function nextEpisode(): Promise<Item | null> {
  const i = it.value; if (!i || i.type !== 'episode' || !i.grandparentRatingKey) return null;
  const all = list<Item>(await get(`/library/metadata/${i.grandparentRatingKey}/allLeaves`).catch(() => null));
  const at = all.findIndex(e => e.id === i.id);
  return at >= 0 ? all.slice(at + 1).find(e => !e.missing) || null : null;
}
let done = false; // the end was reported already (not again when leaving)
// What comes next: the playlist's next, or the next episode (not after a playlist's end).
async function following(): Promise<Item | null> {
  if (route.query.queue) return queue.items[0] || null;
  return upNext.value || await nextEpisode().catch(() => null);
}
function playNext() {
  hideNext();
  const fromQueue = route.query.queue ? nextInQueue() : null;
  if (fromQueue) { router.replace({ name: 'play', params: { id: fromQueue.id }, query: { t: String(Math.floor(resumeAt(fromQueue))), queue: '1' } }); return; }
  const next = route.query.queue ? null : upNext.value;
  if (next) router.replace({ name: 'play', params: { id: next.id }, query: { t: '0' } });
  else close();
}
// The box at the end: "Up next" with a countdown (none when it doesn't play by itself), or "Are you still watching?".
const STILL_WATCHING_AFTER = 3, STILL_WATCHING_WAIT = 30;
const nextBox = ref<{ item: Item, still: boolean, left: number | null } | null>(null);
let countdown = 0;
function showNext(item: Item) {
  clearInterval(countdown);
  const still = autoNext.value && watching.autoRun >= STILL_WATCHING_AFTER;
  nextBox.value = { item, still, left: !autoNext.value ? null : still ? STILL_WATCHING_WAIT : 10 };
  if (nextBox.value.left === null) return;
  countdown = window.setInterval(() => {
    const b = nextBox.value; if (!b || b.left === null) return;
    if (--b.left > 0) return;
    if (b.still) { hideNext(); close(); return; }
    watching.autoRun++;
    playNext();
  }, 1000);
}
function hideNext() { clearInterval(countdown); nextBox.value = null; }
function cancelNext() { hideNext(); if (video.value && video.value.ended) close(); }
const nextName = (n: Item) => (n.type === 'episode' ? `${t('S{n}', { n: n.parentIndex || 0 })}${t('E{n}', { n: n.index || 0 })} · ${n.title}` : n.title);
// Any input (not just moving the mouse) shows that somebody is there.
const someone = () => { watching.autoRun = 0; };
const INPUT = ['pointerdown', 'keydown', 'touchstart', 'wheel'];
function ended() {
  // The next part of a split movie.
  if (split.value && part.value < parts.value.length - 1) { load(partStart(part.value + 1)); return; }
  finish();
}
// The end, or "Skip credits" (which counts as watched to the end): what comes next, or close.
// (Cancelled during the credits, the box comes again at the very end.)
async function finish() {
  if (!done) { done = true; await report('stopped', Math.floor(duration.value * 1000)); }
  const next = await following();
  if (!next) { close(); return; }
  if (!route.query.queue) upNext.value = next;
  showNext(next);
}
function close() {
  if (window.history.state && window.history.state.back) router.back();
  else router.replace(it.value ? '/item/' + it.value.id : '/');
}

function wake() {
  idle.value = false;
  clearTimeout(idleTimer);
  idleTimer = window.setTimeout(() => { if (!paused.value && !menu.value) idle.value = true; }, 3000);
}
function key(e: KeyboardEvent) {
  if (e.ctrlKey || e.metaKey || e.altKey || (e.target as HTMLElement).tagName === 'INPUT' && (e.target as HTMLInputElement).type !== 'range') return;
  const v = video.value; if (!v) return;
  const k = e.key;
  if (k === ' ' || k === 'k') toggle();
  else if (k === 'ArrowLeft' || k === 'j') skip(-10);
  else if (k === 'ArrowRight' || k === 'l') skip(30);
  else if (k === 'ArrowUp') setVolume(Math.min(1, v.volume + 0.1));
  else if (k === 'ArrowDown') setVolume(Math.max(0, v.volume - 0.1));
  else if (k === 'm') toggleMute();
  else if (k === 'f') toggleFull();
  else if (k === '<' || k === '>') { const n = SPEEDS.indexOf(speed.value) + (k === '>' ? 1 : -1); setSpeed(SPEEDS[Math.max(0, Math.min(SPEEDS.length - 1, n))], true); }
  else if (k === 'Escape' && !document.fullscreenElement) { if (menu.value) menu.value = ''; else if (nextBox.value) cancelNext(); else close(); }
  else return;
  e.preventDefault(); wake();
}
const onFull = () => { full.value = !!document.fullscreenElement; };

onMounted(async () => {
  const v = video.value!;
  try { v.volume = Math.min(1, Math.max(0, Number(localStorage.getItem('volume') || 1))); } catch { /* none */ }
  v.addEventListener('timeupdate', () => { now.value = base.value + offset.value + v.currentTime; });
  v.addEventListener('progress', () => { buffered.value = v.buffered.length ? v.buffered.end(v.buffered.length - 1) : 0; });
  v.addEventListener('play', () => { paused.value = false; wake(); if (sound) sound.ctx.resume().catch(() => {}); });
  // The browser's own menu (picture-in-picture …) can change the speed too: keep it in step.
  v.addEventListener('ratechange', () => { if (SPEEDS.includes(v.playbackRate) && v.playbackRate !== speed.value) setSpeed(v.playbackRate); });
  v.addEventListener('pause', () => { paused.value = true; idle.value = false; if (!v.ended) report('paused'); });
  v.addEventListener('waiting', () => { waiting.value = true; });
  v.addEventListener('playing', () => { waiting.value = false; });
  v.addEventListener('canplay', () => { waiting.value = false; });
  v.addEventListener('volumechange', () => { muted.value = v.muted; volume.value = v.volume; });
  v.addEventListener('ended', ended);
  v.addEventListener('error', () => { if (v.src) { waiting.value = false; error.value = t('This video can’t be played here.'); } });
  document.addEventListener('keydown', key);
  document.addEventListener('fullscreenchange', onFull);
  for (const ev of INPUT) document.addEventListener(ev, someone, { passive: true, capture: true });
  try {
    it.value = list<Item>(await get('/library/metadata/' + props.id))[0] || null;
    if (!it.value) { error.value = t('Not found'); return; }
    chooseTracks();
    await load(Number(route.query.t) || 0);
    const id = it.value.id;
    if (media.value) get<Container<'Chapter', { start: number, title: string }>>(`/library/metadata/${id}/chapters`).then(r => {
      chapters.value = (r.MediaContainer.Chapter || []).map(c => ({ at: c.start / 1000, title: c.title.replace(/^Chapter (\d+)$/, (_, n) => t('Chapter {n}', { n })) }));
    }).catch(() => {});
    if (!route.query.queue) nextEpisode().then(n => { upNext.value = n; }).catch(() => {});
    timer = window.setInterval(() => { if (!paused.value) report('playing'); }, 10000);
  } catch (e) { error.value = (e as Error).message; waiting.value = false; }
});
onBeforeUnmount(() => {
  clearInterval(timer); clearTimeout(idleTimer); clearTimeout(noticeTimer); clearTimeout(freshTimer); clearInterval(countdown);
  for (const ev of INPUT) document.removeEventListener(ev, someone, { capture: true });
  if (sound) sound.ctx.close().catch(() => {});
  document.removeEventListener('keydown', key);
  document.removeEventListener('fullscreenchange', onFull);
  if (it.value && !stopped.value && !done) report('stopped');
  const v = video.value;
  if (v) { v.pause(); v.removeAttribute('src'); v.load(); }
  if (document.fullscreenElement) document.exitFullscreen().catch(() => {});
});

const pct = computed(() => (duration.value ? (now.value / duration.value) * 100 : 0));
const bufPct = computed(() => (duration.value ? Math.min(100, ((base.value + offset.value + buffered.value) / duration.value) * 100) : 0));
// The lines move with the controls.
const bottom = ref<HTMLElement | null>(null);
watch(idle, () => placeCues());
const methodName = computed(() => ({ direct: t('direct play'), remux: t('repackaged'), transcode: t('converting') }[method.value]));
// A click on the time switches to the time left (kept with the account); while it plays, the top says when it ends.
const timeLeft = computed(() => pref('timeLeft', '0') === '1');
const timeText = computed(() => (timeLeft.value && duration.value ? '−' + fmtTime(duration.value - now.value) : `${fmtTime(now.value)} / ${fmtTime(duration.value)}`));
const endsAt = computed(() => {
  if (paused.value || !duration.value) return '';
  return t('ends at {time}', { time: fmtClock(new Date(Date.now() + ((duration.value - now.value) / (speed.value || 1)) * 1000)) });
});
</script>

<template>
  <div ref="box" class="player" :class="{ idle }" :style="cueStyle" @mousemove="wake" @click.self="toggle">
    <video ref="video" playsinline @click="toggle"></video>
    <div v-if="waiting && !error" class="spinner" aria-hidden="true"></div>
    <div v-if="error || stopped" class="p-msg">
      <h2>{{ stopped ? t('Playback stopped') : error }}</h2>
      <p v-if="stopped">{{ stoppedMsg || t('The owner of this server stopped your playback.') }}</p>
      <button class="primary" @click="close">{{ t('Back') }}</button>
    </div>
    <div class="p-top">
      <button class="iconbtn" :aria-label="t('Back')" @click="close"><Icon name="back" /></button>
      <div>
        <div class="p-title">{{ title }}</div>
        <div class="dim">{{ subLine }}<span v-if="it"> · {{ methodName }}</span><span v-if="endsAt"> · {{ endsAt }}</span></div>
      </div>
    </div>
    <div v-if="notice" class="p-notice">{{ notice }}</div>
    <div v-if="nextBox" class="p-next" @click.stop>
      <img :src="picture(nextBox.item.thumb || `/library/metadata/${nextBox.item.id}/thumb`, 300)" alt="">
      <div>
        <template v-if="nextBox.still">
          <b>{{ t('Are you still watching?') }}</b>
          <div class="dim">{{ nextName(nextBox.item) }} · {{ t('stops in {n} s', { n: nextBox.left ?? 0 }) }}</div>
        </template>
        <template v-else>
          <div class="dim">{{ t('Up next') }}<template v-if="nextBox.left !== null"> · {{ t('in {n} s', { n: nextBox.left }) }}</template></div>
          <b>{{ nextBox.item.type === 'episode' ? nextBox.item.grandparentTitle : nextBox.item.title }}</b>
          <div v-if="nextBox.item.type === 'episode'">{{ nextName(nextBox.item) }}</div>
        </template>
        <div class="actions">
          <button class="primary" @click="playNext">{{ nextBox.still ? t('Yes, keep watching') : t('Play now') }}</button>
          <button @click="nextBox.still ? (hideNext(), close()) : cancelNext()">{{ nextBox.still ? t('Stop') : t('Cancel') }}</button>
        </div>
      </div>
    </div>
    <button v-if="skipBtn && !nextBox && !error && !stopped" class="p-skip" :class="{ shown: skipFresh || !idle }" @click.stop="doSkip">{{ skipBtn.label }}</button>
    <div ref="bottom" class="p-bottom" @click.stop>
      <div class="seek" @mousemove="hover" @mouseleave="tip = null">
        <div class="seek-buf" :style="{ width: bufPct + '%' }"></div>
        <div class="marks">
          <div v-for="c in chapters.length > 1 ? chapters.slice(1) : []" :key="'c' + c.at" class="mark chap" :style="{ left: at(c.at) + '%' }"></div>
          <div v-for="c in credits" :key="'e' + c" class="mark" :style="{ left: at(c) + '%' }"></div>
          <template v-for="m in [intro, recap]" :key="m ? 'i' + m[0] : ''"><div v-if="m" class="mark span" :style="{ left: at(m[0]) + '%', width: Math.max(0.3, at(m[1]) - at(m[0])) + '%' }"></div></template>
        </div>
        <div v-if="tip" class="seek-tip" :style="{ left: tip.x + 'px' }">{{ tip.text }}</div>
        <input type="range" min="0" :max="duration || 1" step="1" :value="now" :aria-label="t('Position')"
          :style="{ '--p': pct + '%' }" @change="seek(Number(($event.target as HTMLInputElement).value))">
      </div>
      <div class="p-controls">
        <button class="iconbtn" :aria-label="paused ? t('Play') : t('Pause')" @click="toggle"><Icon :name="paused ? 'play' : 'pause'" /></button>
        <button class="iconbtn" :aria-label="t('Back 10 seconds')" @click="skip(-10)"><Icon name="rew" /></button>
        <button class="iconbtn" :aria-label="t('Forward 30 seconds')" @click="skip(30)"><Icon name="fwd" /></button>
        <button class="iconbtn" :aria-label="muted ? t('Sound on') : t('Mute')" @click="toggleMute"><Icon :name="muted || volume === 0 ? 'mute' : 'vol'" /></button>
        <input class="vol" type="range" min="0" max="1" step="0.05" :value="muted ? 0 : volume" :aria-label="t('Volume')" @input="setVolume(Number(($event.target as HTMLInputElement).value))">
        <button class="time" :title="t('Time left')" @click="savePref('timeLeft', timeLeft ? '0' : '1').catch(() => {})">{{ timeText }}</button>
        <span class="grow"></span>
        <div v-if="chapters.length > 1" class="p-menu-wrap">
          <button class="iconbtn label" :class="{ on: menu === 'chapters' }" @click="menu = menu === 'chapters' ? '' : 'chapters'">{{ t('Chapters') }}</button>
          <div v-if="menu === 'chapters'" class="p-menu">
            <button v-for="c in chapters" :key="c.at" :class="{ on: chapterAt(now) === c }" @click="menu = ''; seek(c.at)">{{ c.title }} <span class="dim">· {{ fmtTime(c.at) }}</span></button>
          </div>
        </div>
        <div class="p-menu-wrap">
          <button v-if="audios.length > 1" class="iconbtn label" :class="{ on: menu === 'audio' }" @click="menu = menu === 'audio' ? '' : 'audio'">{{ t('Audio') }}</button>
          <div v-if="menu === 'audio'" class="p-menu">
            <button v-for="(a, i) in audios" :key="i" :class="{ on: (audio === null ? 0 : audios.findIndex(x => x.index === audio)) === i }" @click="setAudio(a, i)">{{ audioName(a) }}</button>
          </div>
        </div>
        <div class="p-menu-wrap">
          <button v-if="subs.length" class="iconbtn" :class="{ on: sub >= 0 || burn !== null || menu === 'subs' }" :aria-label="t('Subtitles')" @click="menu = menu === 'subs' ? '' : 'subs'"><Icon name="cc" /></button>
          <div v-if="menu === 'subs'" class="p-menu">
            <button :class="{ on: sub < 0 && burn === null }" @click="setSub(-1)">{{ t('Off') }}</button>
            <button v-for="(s, i) in subs" :key="i" :class="{ on: sub === i || burn === i }" :disabled="part > 0 && !isPicture(s)" @click="setSub(i)">
              {{ subName(s, i) }}<template v-if="isPicture(s)"> · {{ t('burned in') }}</template>
            </button>
            <hr>
            <div class="p-menu-head">{{ t('Subtitle size') }}</div>
            <button v-for="[v, l] in SUB_SIZES" :key="v" :class="{ on: pref('subSize', '100') === v }" @click="setSubSize(v)">{{ t(l) }}</button>
          </div>
        </div>
        <div class="p-menu-wrap">
          <button class="iconbtn" :class="{ on: menu === 'settings' }" :aria-label="t('Settings')" @click="page = 'main'; menu = menu === 'settings' ? '' : 'settings'"><Icon name="settings" /></button>
          <div v-if="menu === 'settings' && page === 'main'" class="p-menu">
            <button class="entry" @click="page = 'quality'">{{ t('Quality') }} <span class="dim">{{ qualityName(quality) }} ›</span></button>
            <button class="entry" @click="page = 'speed'">{{ t('Playback speed') }} <span class="dim">{{ speedName(speed) }} ›</span></button>
            <hr>
            <button v-if="hasNext" :class="{ on: autoNext }" @click="savePref('autoNext', autoNext ? '0' : '1').catch(() => {})">{{ autoNext ? '✓ ' : '' }}{{ t('Play the next episode by itself') }}</button>
            <button :class="{ on: night }" @click="setNight(!night)">{{ night ? '✓ ' : '' }}{{ t('Night mode') }}</button>
          </div>
          <div v-else-if="menu === 'settings' && page === 'quality'" class="p-menu">
            <button class="p-menu-head back" @click="page = 'main'">‹ {{ t('Quality') }}</button>
            <button v-for="[v] in QUALITIES" :key="v" :class="{ on: quality === v }" @click="setQuality(v)">{{ qualityName(v) }}</button>
          </div>
          <div v-else-if="menu === 'settings'" class="p-menu">
            <button class="p-menu-head back" @click="page = 'main'">‹ {{ t('Playback speed') }}</button>
            <button v-for="x in SPEEDS" :key="x" :class="{ on: speed === x }" @click="menu = ''; setSpeed(x)">{{ speedName(x) }}</button>
          </div>
        </div>
        <button class="iconbtn" :aria-label="t('Full screen')" @click="toggleFull"><Icon :name="full ? 'exitfull' : 'full'" /></button>
      </div>
    </div>
  </div>
</template>
