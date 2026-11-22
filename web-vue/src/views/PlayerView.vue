<script setup lang="ts">
// The player: the whole window, own controls over a <video>. The server decides how a file plays
// (GET …/decision): as it is (the file itself, the browser seeks in it), or repackaged / converted as a
// stream that starts at the seek point (stream.mp4?offset=…), so one seek bar covers the whole video either way.
// Progress goes to the server every 10 s and on pause and leaving (/:/timeline). An episode goes on with the next.
import { computed, onBeforeUnmount, onMounted, ref } from 'vue';
import { useRoute, useRouter } from 'vue-router';
import { api, get, list, session, ticket, withTicket, type Item, type Stream, type Subtitle } from '../api';
import { fmtTime, langName, lang2, resumeAt } from '../format';
import { t } from '../i18n';
import Icon from '../components/Icon.vue';
import { nextInQueue, queue } from '../queue';

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
const menu = ref<'' | 'subs' | 'audio'>('');
const audio = ref<number | null>(null); // the audio stream's index, null = the file's first
const sub = ref(-1);
const stopped = ref(false); // the owner stopped this playback (from the dashboard)
const stoppedMsg = ref('');
let tk = '';
let timer = 0;
let idleTimer = 0;
let loadSeq = 0;

const PICTURE_SUBS = ['pgs', 'dvd_sub', 'dvb_sub', 'xsub', 'vobsub', 'hdmv'];
const media = computed(() => (it.value && it.value.Media && it.value.Media[0]) || null);
const duration = computed(() => ((it.value && it.value.duration) || 0) / 1000 || (video.value && isFinite(video.value.duration) ? video.value.duration + offset.value : 0));
const audios = computed<Stream[]>(() => ((media.value && media.value.streams) || []).filter(s => s.kind === 'audio'));
// Subtitles made of pictures have to be burned into the picture; the classic app does that, this one leaves them out.
const subs = computed<Subtitle[]>(() => ((media.value && media.value.subtitles) || []).filter(s => !(s.embedded && PICTURE_SUBS.some(k => String(s.codec || '').toLowerCase().includes(k)))));
const title = computed(() => (it.value ? (it.value.type === 'episode' ? it.value.grandparentTitle : it.value.title) : ''));
const subLine = computed(() => {
  const i = it.value; if (!i) return '';
  return i.type === 'episode' ? `${t('S{n}', { n: i.parentIndex || 0 })}${t('E{n}', { n: i.index || 0 })} · ${i.title}` : i.year ? String(i.year) : '';
});
const prefs = () => (session.me && session.me.prefs) || {};
const subName = (s: Subtitle, i: number) => [s.language ? langName(s.language) : '', s.title, s.forced ? t('forced') : '', s.sdh ? 'SDH' : ''].filter(Boolean).join(' · ') || t('Subtitles {n}', { n: i + 1 });
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
  let q = prefs().quality || 'original';
  const want = Number((/^(\d+)p/.exec(q) || [])[1]) || 0, have = Number(media.value && media.value.height) || 0;
  if (want && have && have <= want && prefs().keepSmall !== '0') q = 'original';
  const extra = (CODECS ? `&videoCodecs=${CODECS}` : '') + (audio.value !== null ? `&audio=${audio.value}` : '');
  const d = await get<{ method: 'direct' | 'remux' | 'transcode', allowed: boolean }>(`/library/metadata/${i.id}/decision?quality=${q}${extra}`);
  tk = await ticket('item=' + i.id);
  if (seq !== loadSeq) return;
  let m = audio.value !== null && d.method === 'direct' ? 'remux' : d.method;
  const noDirect = prefs().directPlay === '0' && m === 'direct' && d.allowed;
  if (noDirect) m = 'transcode';
  method.value = m;
  if (m === 'direct') {
    offset.value = 0;
    v.src = withTicket(`/library/parts/${i.id}/file`, tk);
    v.addEventListener('loadedmetadata', () => { v.currentTime = at; }, { once: true });
  } else {
    offset.value = at;
    v.src = withTicket(`/video/transcode/${i.id}/stream.mp4?quality=${q}&offset=${at.toFixed(1)}${extra}${noDirect ? '&transcode=1' : ''}`, tk);
  }
  addSubtitles();
  v.play().catch(() => { waiting.value = false; paused.value = true; });
}

// Text subtitles as <track>s; a converted stream starts at the seek point, so their times move by as much.
function addSubtitles() {
  const v = video.value; if (!v) return;
  v.querySelectorAll('track').forEach(tr => tr.remove());
  subs.value.forEach((s, i) => {
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
  Array.from(v.textTracks).forEach((tr, i) => { tr.mode = i === sub.value ? 'showing' : 'disabled'; });
}
function setSub(i: number) {
  sub.value = i; menu.value = '';
  showSub();
  const s = subs.value[i];
  api('POST', '/api/me/prefs', { subLang: s ? s.language || '' : '' }).catch(() => {});
}
function setAudio(a: Stream, i: number) {
  menu.value = '';
  audio.value = i === 0 ? null : (a.index ?? null);
  load(now.value);
}

function seek(to: number) {
  const v = video.value; if (!v) return;
  to = Math.max(0, Math.min(duration.value ? duration.value - 1 : to, to));
  now.value = to;
  if (method.value === 'direct') { v.currentTime = to; return; }
  // Converted: within what has arrived already the stream itself seeks, else a new stream from there.
  const local = to - offset.value;
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

function report(state: 'playing' | 'paused' | 'stopped') {
  const i = it.value; if (!i) return Promise.resolve();
  return api<{ stopped?: boolean, message?: string } | null>('POST', `/:/timeline?ratingKey=${i.id}&time=${Math.floor(now.value * 1000)}&state=${state}&client=web`)
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
async function ended() {
  done = true;
  await report('stopped');
  const fromQueue = nextInQueue();
  if (fromQueue) { router.replace({ name: 'play', params: { id: fromQueue.id }, query: { t: String(Math.floor(resumeAt(fromQueue))), queue: '1' } }); return; }
  // The end of a playlist: it stops there, also after an episode.
  const next = route.query.queue ? null : await nextEpisode();
  if (next) router.replace({ name: 'play', params: { id: next.id }, query: { t: '0' } });
  else close();
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
  else if (k === 'Escape' && !document.fullscreenElement) { if (menu.value) menu.value = ''; else close(); }
  else return;
  e.preventDefault(); wake();
}
const onFull = () => { full.value = !!document.fullscreenElement; };

onMounted(async () => {
  const v = video.value!;
  try { v.volume = Math.min(1, Math.max(0, Number(localStorage.getItem('volume') || 1))); } catch { /* none */ }
  v.addEventListener('timeupdate', () => { now.value = offset.value + v.currentTime; });
  v.addEventListener('progress', () => { buffered.value = v.buffered.length ? v.buffered.end(v.buffered.length - 1) : 0; });
  v.addEventListener('play', () => { paused.value = false; wake(); });
  v.addEventListener('pause', () => { paused.value = true; idle.value = false; if (!v.ended) report('paused'); });
  v.addEventListener('waiting', () => { waiting.value = true; });
  v.addEventListener('playing', () => { waiting.value = false; });
  v.addEventListener('canplay', () => { waiting.value = false; });
  v.addEventListener('volumechange', () => { muted.value = v.muted; volume.value = v.volume; });
  v.addEventListener('ended', ended);
  v.addEventListener('error', () => { if (v.src) { waiting.value = false; error.value = t('This video can’t be played here.'); } });
  document.addEventListener('keydown', key);
  document.addEventListener('fullscreenchange', onFull);
  try {
    it.value = list<Item>(await get('/library/metadata/' + props.id))[0] || null;
    if (!it.value) { error.value = t('Not found'); return; }
    // The language chosen last time: its audio track, and subtitles in the language picked last.
    const p = prefs();
    if (p.audioLang) { const at = audios.value.findIndex(a => lang2(a.language) === p.audioLang); if (at > 0) audio.value = audios.value[at].index ?? null; }
    if (p.subLang) sub.value = subs.value.findIndex(s => (s.language || '') === p.subLang && !s.forced);
    await load(Number(route.query.t) || 0);
    timer = window.setInterval(() => { if (!paused.value) report('playing'); }, 10000);
  } catch (e) { error.value = (e as Error).message; waiting.value = false; }
});
onBeforeUnmount(() => {
  clearInterval(timer); clearTimeout(idleTimer);
  document.removeEventListener('keydown', key);
  document.removeEventListener('fullscreenchange', onFull);
  if (it.value && !stopped.value && !done) report('stopped');
  const v = video.value;
  if (v) { v.pause(); v.removeAttribute('src'); v.load(); }
  if (document.fullscreenElement) document.exitFullscreen().catch(() => {});
});

const pct = computed(() => (duration.value ? (now.value / duration.value) * 100 : 0));
const bufPct = computed(() => (duration.value ? Math.min(100, ((offset.value + buffered.value) / duration.value) * 100) : 0));
const methodName = computed(() => ({ direct: t('direct play'), remux: t('repackaged'), transcode: t('converting') }[method.value]));
</script>

<template>
  <div ref="box" class="player" :class="{ idle }" @mousemove="wake" @click.self="toggle">
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
        <div class="dim">{{ subLine }}<span v-if="it"> · {{ methodName }}</span></div>
      </div>
    </div>
    <div class="p-bottom" @click.stop>
      <div class="seek">
        <div class="seek-buf" :style="{ width: bufPct + '%' }"></div>
        <input type="range" min="0" :max="duration || 1" step="1" :value="now" :aria-label="t('Position')"
          :style="{ '--p': pct + '%' }" @change="seek(Number(($event.target as HTMLInputElement).value))">
      </div>
      <div class="p-controls">
        <button class="iconbtn" :aria-label="paused ? t('Play') : t('Pause')" @click="toggle"><Icon :name="paused ? 'play' : 'pause'" /></button>
        <button class="iconbtn" :aria-label="t('Back 10 seconds')" @click="skip(-10)"><Icon name="rew" /></button>
        <button class="iconbtn" :aria-label="t('Forward 30 seconds')" @click="skip(30)"><Icon name="fwd" /></button>
        <button class="iconbtn" :aria-label="muted ? t('Sound on') : t('Mute')" @click="toggleMute"><Icon :name="muted || volume === 0 ? 'mute' : 'vol'" /></button>
        <input class="vol" type="range" min="0" max="1" step="0.05" :value="muted ? 0 : volume" :aria-label="t('Volume')" @input="setVolume(Number(($event.target as HTMLInputElement).value))">
        <span class="time">{{ fmtTime(now) }} / {{ fmtTime(duration) }}</span>
        <span class="grow"></span>
        <div class="p-menu-wrap">
          <button v-if="audios.length > 1" class="iconbtn label" :class="{ on: menu === 'audio' }" @click="menu = menu === 'audio' ? '' : 'audio'">{{ t('Audio') }}</button>
          <div v-if="menu === 'audio'" class="p-menu">
            <button v-for="(a, i) in audios" :key="i" :class="{ on: (audio === null ? 0 : audios.findIndex(x => x.index === audio)) === i }" @click="setAudio(a, i)">{{ audioName(a) }}</button>
          </div>
        </div>
        <div class="p-menu-wrap">
          <button v-if="subs.length" class="iconbtn" :class="{ on: sub >= 0 || menu === 'subs' }" :aria-label="t('Subtitles')" @click="menu = menu === 'subs' ? '' : 'subs'"><Icon name="cc" /></button>
          <div v-if="menu === 'subs'" class="p-menu">
            <button :class="{ on: sub < 0 }" @click="setSub(-1)">{{ t('Off') }}</button>
            <button v-for="(s, i) in subs" :key="i" :class="{ on: sub === i }" @click="setSub(i)">{{ subName(s, i) }}</button>
          </div>
        </div>
        <button class="iconbtn" :aria-label="t('Full screen')" @click="toggleFull"><Icon :name="full ? 'exitfull' : 'full'" /></button>
      </div>
    </div>
  </div>
</template>
