<script setup lang="ts">
// The dashboard (owner): what plays now (and stopping it with a message), the server's load as tiles,
// who was seen lately, charts of bandwidth, processor, memory and streams over a chosen time, and the play
// history (per day or week, the most played titles, the top users, the latest viewings).
import { computed, onBeforeUnmount, onMounted, ref, watch } from 'vue';
import { api, get, picture } from '../api';
import { fmtAgo, fmtBits, fmtBytes, fmtDateTime, fmtDur, fmtNum, fmtPct, fmtTime, fmtUptime, langName } from '../format';
import { lang, t } from '../i18n';
import { toast } from '../toast';
import BarChart, { type Bucket } from '../components/BarChart.vue';
import LineChart, { type Series } from '../components/LineChart.vue';
import Icon from '../components/Icon.vue';
import Modal from '../components/Modal.vue';

interface Video { height?: number, codec?: string, profile?: string, hdr?: boolean, action?: string, out_height?: number, out_codec?: string, hw?: boolean, /** Converted on the transcode helper. */ helper?: boolean }
interface Audio { language?: string, title?: string, codec?: string, channels?: number, action?: string, out_codec?: string, out_channels?: number }
interface Playing {
  id: number, type: string, title?: string, grandparentTitle?: string, parentIndex?: number, index?: number, year?: number, thumb?: string,
  user?: string, user_id: number, client?: string, local?: boolean, ip?: string, out_bps?: number, method?: string, height?: number,
  state?: string, offset: number, duration?: number, video?: Video, audio?: Audio,
}
interface User { id: number, username: string, display_name?: string, admin?: boolean, active?: boolean, last_seen?: number, client?: string, ip?: string, playing?: number, out_bps?: number, last_play?: number }
interface Disk { free_percent: number, free_bytes: number, total_bytes: number, holds: string[], paths: string[] }
interface Dash {
  now: number, started_at: number, version: string, build?: string, bytes_sent: number, sample_seconds: number, cores: number,
  max_transcodes?: number, active_seconds: number, disk_warn_percent?: number, disks?: Disk[], playing: Playing[], users: User[],
  current?: { cpu?: number, cpu_server?: number | null, mem_used?: number, mem_total?: number, mem_server?: number, out_bps?: number, out_remote_bps?: number },
}
interface Sample { at: number, cpu?: number, cpu_server?: number, mem_used?: number, mem_server?: number, out_bps?: number, out_remote_bps?: number, streams?: number, transcodes?: number }
interface HistItem { id: number, type: string, title?: string, grandparentTitle?: string, parentIndex?: number, index?: number }
interface History {
  plays: [number, number, 'movie' | 'episode' | 'other', number][], size: number, minutes: number,
  titles: { id: number, title?: string, year?: number, type: string, thumb?: string, plays: number }[],
  users: { username: string, display_name?: string, plays: number, minutes: number }[],
  recent: { at: number, user?: string, item: HistItem, minutes: number, manual?: boolean, live?: boolean }[],
}

const shownName = (u: { username: string, display_name?: string }) => u.display_name || u.username || '?';
const d = ref<Dash | null>(null);
const error = ref('');
let timer = 0, alive = true;
async function live() {
  clearTimeout(timer);
  try { d.value = await get<Dash>('/api/dashboard'); error.value = ''; } catch (e) { error.value = (e as Error).message; }
  if (!alive) return;
  timer = window.setTimeout(live, ((d.value && d.value.sample_seconds) || 10) * 1000);
  // Real time follows every sample; longer ranges change slowly (once a minute is enough).
  if (range.value === 'realtime' || !series.value || Date.now() - seriesAt > 60000) loadSeries();
}
onMounted(() => { live(); history(); });
onBeforeUnmount(() => { alive = false; clearTimeout(timer); clearInterval(tick); });

// ---- what plays now; the position moves on every second between the server's updates
const now = ref(Date.now());
const tick = window.setInterval(() => { now.value = Date.now(); }, 1000);
let fetchedAt = Date.now();
watch(d, () => { fetchedAt = Date.now(); });
const offset = (p: Playing) => (p.state === 'paused' || !p.duration ? p.offset : Math.min(p.duration, p.offset + (now.value - fetchedAt)));
const CODECS: Record<string, string> = { hevc: 'HEVC', h264: 'H.264', av1: 'AV1', vp9: 'VP9', vp8: 'VP8', mpeg2video: 'MPEG-2', mpeg4: 'MPEG-4', vc1: 'VC-1', ac3: 'AC3', eac3: 'EAC3', dts: 'DTS', truehd: 'TrueHD', aac: 'AAC', flac: 'FLAC', opus: 'Opus', mp3: 'MP3', vorbis: 'Vorbis', pcm_s16le: 'PCM', pcm_s24le: 'PCM' };
const codecName = (c?: string) => CODECS[c || ''] || (c || '').toUpperCase();
const resName = (h?: number) => (!h ? '' : h >= 2000 ? '4K' : h >= 1000 ? '1080p' : h >= 700 ? '720p' : h >= 540 ? '576p' : h >= 470 ? '480p' : 'SD');
const chName = (n?: number) => ({ 1: 'Mono', 2: t('Stereo'), 3: '2.1', 6: '5.1', 7: '6.1', 8: '7.1' } as Record<number, string>)[n || 0] || (n ? n + ' ch' : '');
const methodLabel = (p: Playing) => (p.method === 'transcode' ? t('Transcode') + (p.height ? ` ${p.height}p` : '') : p.method === 'remux' ? t('Direct stream') : t('Direct play'));
// "1080p (HEVC Main 10)" → "720p (H.264) — Transcode (hw)", like the player sees it.
function videoLine(v?: Video): [string, string, boolean] | null {
  if (!v) return null;
  const from = `${resName(v.height)} (${[codecName(v.codec), v.profile, v.hdr ? 'HDR' : ''].filter(Boolean).join(' ')})`;
  return v.action === 'transcode' ? [from, ` → ${resName(v.out_height)} (${codecName(v.out_codec)}) — ${t('Transcode')}${v.hw ? ' (hw)' : ''}${v.helper ? ' · ' + t('on the transcode helper') : ''}`, false] : [from, ' — ' + t('Direct'), true];
}
function audioLine(a?: Audio): [string, string, boolean] | null {
  if (!a) return null;
  const from = `${a.language ? langName(a.language) : a.title || t('Unknown')} (${[codecName(a.codec), chName(a.channels)].filter(Boolean).join(' ')})`;
  return a.action === 'transcode' ? [from, ` → ${codecName(a.out_codec)} ${chName(a.out_channels)} — ${t('Transcode')}`, false] : [from, ' — ' + t('Direct'), true];
}
const pTitle = (p: Playing) => (p.type === 'episode' && p.grandparentTitle ? p.grandparentTitle : p.title || t('(removed)'));
const pSub = (p: Playing) => (p.type === 'episode' ? `${t('S{n}', { n: p.parentIndex || 0 })} ${t('E{n}', { n: p.index || 0 })} · ${p.title}` : String(p.year || ''));
const pWhere = (p: Playing) => (p.local ? t('Local') : t('Remote')) + (p.ip ? ` (${p.ip})` : '') + ' — ' + fmtBits(p.out_bps || 0);

const stopping = ref<Playing | null>(null);
const stopMsg = ref('');
const stopBusy = ref(false);
async function stop() {
  const p = stopping.value; if (!p) return;
  stopBusy.value = true;
  try { await api('POST', '/api/dashboard/stop', { user: p.user_id, item: p.id, message: stopMsg.value }); toast(t('Playback stopped')); stopping.value = null; stopMsg.value = ''; live(); }
  catch (e) { toast((e as Error).message, 6000); } finally { stopBusy.value = false; }
}

// ---- the tiles
const c = computed(() => (d.value && d.value.current) || {});
const transcodes = computed(() => (d.value ? d.value.playing.filter(p => p.method === 'transcode').length : 0));
const activeUsers = computed(() => (d.value ? d.value.users.filter(u => u.active).length : 0));
const subline = computed(() => d.value && t('Running for {time} · version {v} · {sent} sent since the start · updates every {n} s', { time: fmtUptime(d.value.now - d.value.started_at), v: d.value.version + (d.value.build ? ' (' + d.value.build + ')' : ''), sent: fmtBytes(d.value.bytes_sent), n: d.value.sample_seconds }));
// The fullest disk of the libraries and the data folder; the others in its tooltip.
const disk = computed(() => {
  const list = [...((d.value && d.value.disks) || [])].sort((a, b) => a.free_percent - b.free_percent);
  if (!list.length) return null;
  const holds = (x: Disk) => x.holds.map(w => (w === 'data' ? t('data folder') : w)).join(', ');
  const line = (x: Disk) => t('{free} free of {total}', { free: fmtBytes(x.free_bytes), total: fmtBytes(x.total_bytes) }) + ' · ' + holds(x);
  return {
    value: fmtPct(list[0].free_percent), low: !!(d.value!.disk_warn_percent && list[0].free_percent < d.value!.disk_warn_percent),
    note: line(list[0]) + (list.length > 1 ? ' · ' + t('{n} more disks', { n: list.length - 1 }) : ''),
    tip: list.map(x => fmtPct(x.free_percent) + ' — ' + line(x) + '\n' + x.paths.join('\n')).join('\n\n'),
  };
});

// ---- the charts over a time range (kept in this browser)
const RANGES: [string, string][] = [['realtime', 'Real time'], ['12h', 'Last 12 hours'], ['24h', 'Last 24 hours'], ['7d', 'Last 7 days'], ['30d', 'Last 30 days'], ['90d', 'Last 90 days'], ['365d', 'Last year'], ['all', 'Unlimited']];
const SHOW: [string, string][] = [['all', 'All'], ['server', 'Lichtspiel'], ['system', 'System']];
const range = ref(localStorage.getItem('vue-dashRange') || 'realtime');
const cpuShow = ref(localStorage.getItem('vue-dashCpu') || 'all');
const memShow = ref(localStorage.getItem('vue-dashMem') || 'all');
watch(range, v => { localStorage.setItem('vue-dashRange', v); loadSeries(); });
watch(cpuShow, v => localStorage.setItem('vue-dashCpu', v));
watch(memShow, v => localStorage.setItem('vue-dashMem', v));
const series = ref<{ range: string, samples: Sample[] } | null>(null);
let seriesAt = 0;
async function loadSeries() {
  try { series.value = await get('/api/dashboard/stats?range=' + range.value); seriesAt = Date.now(); } catch { /* the next round tries again */ }
}
const charts = computed(() => {
  const ss = (series.value && series.value.samples) || [];
  const times = ss.map(s => s.at);
  const dates = ss.length > 1 && times[times.length - 1] - times[0] > 36 * 3600;
  const label = t((RANGES.find(x => x[0] === range.value) || RANGES[0])[1]);
  const num = (v?: number | null) => (v === null || v === undefined ? null : v);
  const gb = (v?: number | null) => (v === null || v === undefined ? null : v / 1e9);
  // The unit follows the highest value of the range.
  const peak = Math.max(0, ...ss.map(s => (s.out_bps || 0) * 8));
  const [div, unit] = peak >= 1e9 ? [1e9, 'Gbit/s'] : peak >= 1e6 ? [1e6, 'Mbit/s'] : peak >= 1e3 ? [1e3, 'kbit/s'] : [1, 'bit/s'];
  const bits = (v: number) => (v * 8) / div;
  const pick = (show: string, sys: Series, srv: Series) => [show !== 'server' ? sys : null, show !== 'system' ? srv : null].filter((x): x is Series => !!x);
  const memMax = c.value.mem_total ? c.value.mem_total / 1e9 : null;
  return {
    times, dates, label, unit,
    bw: [
      { name: t('Local'), color: 'var(--s1)', fill: true, values: ss.map(s => (s.out_bps === undefined ? null : bits(Math.max(0, s.out_bps - (s.out_remote_bps || 0))))) },
      { name: t('Remote'), color: 'var(--s2)', fill: true, values: ss.map(s => (s.out_bps === undefined ? null : bits(s.out_remote_bps || 0))) },
    ] as Series[],
    bwFmt: (v: number, full?: boolean) => fmtNum(v, v && v < 10 && v % 1 ? 1 : 0) + (full ? ' ' + unit : ''),
    cpu: pick(cpuShow.value, { name: t('System'), color: 'var(--s1)', values: ss.map(s => num(s.cpu)) }, { name: 'Lichtspiel', color: 'var(--s2)', values: ss.map(s => num(s.cpu_server)) }),
    cpuMax: cpuShow.value === 'server' ? null : 100,
    mem: pick(memShow.value, { name: t('System'), color: 'var(--s1)', values: ss.map(s => gb(s.mem_used)) }, { name: 'Lichtspiel', color: 'var(--s2)', values: ss.map(s => gb(s.mem_server)) }),
    memMax: memShow.value === 'server' ? null : memMax,
    streams: [{ name: t('Playing'), color: 'var(--s1)', values: ss.map(s => num(s.streams)) }, { name: t('Converted'), color: 'var(--s2)', values: ss.map(s => num(s.transcodes)) }] as Series[],
    streamsMax: Math.max(4, ...ss.map(s => s.streams || 0)),
  };
});
const pctFmt = (v: number) => fmtNum(v, 0) + ' %';
const gbFmt = (v: number, full?: boolean) => fmtNum(v, full && v < 10 ? 1 : 0) + (full ? ' GB' : '');
const countFmt = (v: number) => fmtNum(v, 0);

// ---- the play history: viewings per day (or per week for long spans), in this browser's time zone
const days = ref(30);
const metric = ref<'plays' | 'hours'>('plays');
const user = ref('');
const hist = ref<History | null>(null);
const histError = ref('');
async function history() {
  try { hist.value = await get<History>(`/api/dashboard/history?days=${days.value}${user.value ? '&user=' + user.value : ''}`); histError.value = ''; } catch (e) { histError.value = (e as Error).message; }
}
watch([days, user], history);
const KINDS: [string, string, string][] = [['movie', 'Movies', 'var(--s1)'], ['episode', 'Episodes', 'var(--s2)'], ['other', 'Other videos', 'var(--s3)']];
const weekly = computed(() => days.value > 90);
const buckets = computed<Bucket[]>(() => {
  const r = hist.value; if (!r) return [];
  const day0 = new Date(); day0.setHours(0, 0, 0, 0);
  const start = new Date(day0); start.setDate(start.getDate() - (days.value - 1));
  if (weekly.value) start.setDate(start.getDate() - ((start.getDay() + 6) % 7));
  const out: (Bucket & { from: number, to: number })[] = [];
  for (const x = new Date(start); x <= day0; x.setDate(x.getDate() + (weekly.value ? 7 : 1))) {
    const from = new Date(x), to = new Date(x); to.setDate(to.getDate() + (weekly.value ? 7 : 1));
    out.push({ from: from.getTime() / 1000, to: to.getTime() / 1000, label: from.toLocaleDateString(lang.value, { day: 'numeric', month: 'numeric' }), parts: { movie: 0, episode: 0, other: 0 },
      tip: weekly.value ? t('Week of {date}', { date: from.toLocaleDateString(lang.value) }) : from.toLocaleDateString(lang.value, { weekday: 'long', day: 'numeric', month: 'long', year: 'numeric' }) });
  }
  for (const [at, , kind, mins] of r.plays) {
    const b = out.find(x => at >= x.from && at < x.to); if (!b) continue;
    b.parts[kind] += metric.value === 'hours' ? mins / 60 : 1;
  }
  return out;
});
const histFmt = (v: number, full?: boolean) => (metric.value === 'hours' ? fmtNum(v, v && v < 10 ? 1 : 0) + (full ? ' h' : '') : fmtNum(Math.round(v * 100) / 100, v % 1 ? 1 : 0));
const watchedFor = (mins: number) => fmtDur(mins * 60000) || '0 min';
const what = (it: HistItem) => (it.type === 'episode' && it.grandparentTitle ? `${it.grandparentTitle} · ${t('S{n}', { n: it.parentIndex || 0 })}${t('E{n}', { n: it.index || 0 })} ${it.title}` : it.title || t('(removed)'));
</script>

<template>
  <div class="dash">
    <div class="dash-head"><h1>{{ t('Dashboard') }}</h1><span class="dim">{{ subline }}</span></div>
    <p v-if="error" class="err">{{ error }}</p>
    <div v-if="!d" class="skel line"></div>
    <template v-else>
      <div class="tiles">
        <div class="tile"><div class="dim lbl">{{ t('Playing now') }}</div><div class="val">{{ d.playing.length }}</div>
          <div class="dim note">{{ transcodes ? t('{n} of them converted', { n: transcodes }) + (d.max_transcodes ? ' ' + t('(at most {n} at once)', { n: d.max_transcodes }) : '') : t('none converted') }}</div></div>
        <div class="tile"><div class="dim lbl">{{ t('Bandwidth') }}</div><div class="val">{{ fmtBits(c.out_bps || 0) }}</div>
          <div class="dim note">{{ c.out_bps ? t('{local} local · {remote} remote', { local: fmtBits(Math.max(0, c.out_bps - (c.out_remote_bps || 0))), remote: fmtBits(c.out_remote_bps || 0) }) : t('sent to players right now') }}</div></div>
        <div class="tile"><div class="dim lbl">{{ t('Processor') }}</div><div class="val">{{ fmtPct(c.cpu) }}</div>
          <div class="dim note">{{ c.cpu_server !== null && c.cpu_server !== undefined ? t('Lichtspiel: {pct} · {n} cores', { pct: fmtPct(c.cpu_server), n: d.cores }) : t('{n} cores', { n: d.cores }) }}</div></div>
        <div class="tile"><div class="dim lbl">{{ t('Memory') }}</div><div class="val">{{ c.mem_total ? fmtPct((100 * (c.mem_used || 0)) / c.mem_total) : '–' }}</div>
          <div class="dim note">{{ (c.mem_total ? `${fmtBytes(c.mem_used || 0)} / ${fmtBytes(c.mem_total)}` : '–') + (c.mem_server ? ' · ' + t('Lichtspiel: {size}', { size: fmtBytes(c.mem_server) }) : '') }}</div></div>
        <div class="tile"><div class="dim lbl">{{ t('Active users') }}</div><div class="val">{{ activeUsers }}</div>
          <div class="dim note">{{ t('of {n} · active = seen in the last {m} min', { n: d.users.length, m: Math.round(d.active_seconds / 60) }) }}</div></div>
        <div v-if="disk" class="tile" :title="disk.tip"><div class="dim lbl">{{ t('Disk space') }}</div><div class="val" :class="{ err: disk.low }">{{ disk.value }}</div><div class="dim note">{{ disk.note }}</div></div>
      </div>

      <h2 class="gap-top">{{ t('Playing now') }}</h2>
      <div v-if="d.playing.length" class="np-list">
        <div v-for="p in d.playing" :key="p.user_id + ':' + p.id" class="np">
          <RouterLink :to="'/item/' + p.id" class="np-img"><img :src="picture(p.thumb, 140)" alt="" loading="lazy"></RouterLink>
          <div class="np-body">
            <div class="np-head">
              <div><RouterLink :to="'/item/' + p.id"><b>{{ pTitle(p) }}</b></RouterLink><div class="dim">{{ pSub(p) }}</div></div>
              <button class="iconbtn np-stop" :title="t('Stop playback')" :aria-label="t('Stop playback')" @click="stopping = p"><Icon name="close" /></button>
            </div>
            <div class="np-who"><b>{{ p.user || '?' }}</b><span class="np-method" :class="{ warn: p.method === 'transcode' }">{{ methodLabel(p) }}</span></div>
            <div class="np-rows">
              <span>{{ t('Device') }}</span><span>{{ p.client || '–' }}</span>
              <span>{{ t('Connection') }}</span><span>{{ pWhere(p) }}</span>
              <template v-if="videoLine(p.video)"><span>{{ t('Video') }}</span><span>{{ videoLine(p.video)![0] }}<span :class="{ dim: videoLine(p.video)![2] }">{{ videoLine(p.video)![1] }}</span></span></template>
              <template v-if="audioLine(p.audio)"><span>{{ t('Sound') }}</span><span>{{ audioLine(p.audio)![0] }}<span :class="{ dim: audioLine(p.audio)![2] }">{{ audioLine(p.audio)![1] }}</span></span></template>
            </div>
            <div class="np-prog">
              <span>{{ p.state === 'paused' ? '⏸ ' + t('Paused') : '▶ ' + t('Playing') }}</span>
              <div class="pbar"><i :style="{ width: (p.duration ? Math.min(100, offset(p) / p.duration * 100) : 0) + '%' }"></i></div>
              <span v-if="p.duration" class="dim nowrap">{{ fmtTime(offset(p) / 1000) }} / {{ fmtTime(p.duration / 1000) }}</span>
            </div>
          </div>
        </div>
      </div>
      <p v-else class="empty">{{ t('Nothing is playing right now.') }}</p>

      <h2>{{ t('Users') }}</h2>
      <div class="table-wrap panel">
        <table class="table">
          <tr><th v-for="x in ['User', 'Status', 'Last seen', 'Device', 'Address', 'Playing', 'Bandwidth', 'Last viewing']" :key="x">{{ t(x) }}</th></tr>
          <tr v-for="u in d.users" :key="u.id">
            <td><b :title="u.username">{{ shownName(u) }}</b><span v-if="u.admin" class="tag">{{ t('Admin') }}</span></td>
            <td><span class="status" :class="u.active ? 'on' : 'off'"><i></i>{{ u.active ? t('active') : t('away') }}</span></td>
            <td class="dim">{{ u.last_seen ? fmtAgo(u.last_seen, d.now) : t('not since the start') }}</td>
            <td class="dim">{{ u.client || '–' }}</td><td class="dim">{{ u.ip || '–' }}</td>
            <td>{{ u.playing || '–' }}</td><td class="nowrap">{{ u.out_bps ? fmtBits(u.out_bps) : '–' }}</td>
            <td class="dim">{{ u.last_play ? fmtDateTime(u.last_play) : '–' }}</td>
          </tr>
        </table>
      </div>
    </template>

    <div class="dash-head gap-top">
      <h2>{{ t('Activity') }}</h2>
      <select v-model="range" :aria-label="t('Time range')"><option v-for="[v, l] in RANGES" :key="v" :value="v">{{ t(l) }}</option></select>
    </div>
    <div v-if="series" class="dash-charts">
      <div class="panel"><div class="ch-title"><b>{{ t('Bandwidth') }}</b><span class="dim">{{ charts.label }} · {{ charts.unit }}</span></div>
        <LineChart :times="charts.times" :dates="charts.dates" :series="charts.bw" :fmt="charts.bwFmt" /></div>
      <div class="panel"><div class="ch-title"><b>{{ t('Processor') }}</b><div class="seg"><button v-for="[v, l] in SHOW" :key="v" class="small" :class="{ on: cpuShow === v }" @click="cpuShow = v">{{ t(l) }}</button></div></div>
        <LineChart :times="charts.times" :dates="charts.dates" :series="charts.cpu" :max="charts.cpuMax" :fmt="pctFmt" /></div>
      <div class="panel"><div class="ch-title"><b>{{ t('Memory') }}</b><div class="seg"><button v-for="[v, l] in SHOW" :key="v" class="small" :class="{ on: memShow === v }" @click="memShow = v">{{ t(l) }}</button></div></div>
        <LineChart :times="charts.times" :dates="charts.dates" :series="charts.mem" :max="charts.memMax" :fmt="gbFmt" /></div>
      <div class="panel"><div class="ch-title"><b>{{ t('Streams') }}</b><span class="dim">{{ charts.label }}</span></div>
        <LineChart :times="charts.times" :dates="charts.dates" :series="charts.streams" :max="charts.streamsMax" :fmt="countFmt" /></div>
    </div>

    <div class="dash-head gap-top">
      <h2>{{ t('Play history') }}</h2>
      <div class="hist-tools">
        <div class="seg"><button v-for="[v, l] in ([[7, '7 days'], [30, '30 days'], [90, '90 days'], [365, '1 year']] as [number, string][])" :key="v" class="small" :class="{ on: days === v }" @click="days = v">{{ t(l) }}</button></div>
        <div class="seg"><button v-for="[v, l] in ([['plays', 'Viewings'], ['hours', 'Hours']] as ['plays' | 'hours', string][])" :key="v" class="small" :class="{ on: metric === v }" @click="metric = v">{{ t(l) }}</button></div>
        <select v-model="user" :aria-label="t('User')"><option value="">{{ t('Everyone') }}</option><option v-for="u in (d ? d.users : [])" :key="u.id" :value="String(u.id)">{{ shownName(u) }}</option></select>
      </div>
    </div>
    <p v-if="histError" class="err">{{ histError }}</p>
    <template v-if="hist">
      <div class="panel">
        <div class="ch-title">
          <b>{{ metric === 'hours' ? t('Hours watched per {unit}', { unit: weekly ? t('week') : t('day') }) : t('Viewings per {unit}', { unit: weekly ? t('week') : t('day') }) }}</b>
          <span class="dim">{{ t('{n} viewings · {h} watched', { n: hist.size, h: watchedFor(hist.minutes) }) }}</span>
        </div>
        <BarChart :buckets="buckets" :keys="KINDS" :fmt="histFmt" />
      </div>
      <div class="dash-cols">
        <div class="panel"><b>{{ t('Most played') }}</b>
          <ol v-if="hist.titles.length" class="ranked">
            <li v-for="x in hist.titles" :key="x.id">
              <img :src="picture(x.thumb, 60)" alt="" loading="lazy">
              <div class="grow"><RouterLink :to="'/item/' + x.id">{{ x.title || t('(removed)') }}</RouterLink><div class="dim">{{ [x.year, x.type === 'show' ? t('Show') : null].filter(Boolean).join(' · ') }}</div></div>
              <span class="dim">{{ t('{n}×', { n: x.plays }) }}</span>
            </li>
          </ol>
          <p v-else class="dim">{{ t('No viewings in this time.') }}</p>
        </div>
        <div class="panel"><b>{{ t('Top users') }}</b>
          <ol v-if="hist.users.length" class="ranked">
            <li v-for="x in hist.users" :key="x.username"><div class="grow"><b>{{ shownName(x) }}</b></div><span class="dim">{{ t('{n} viewings · {h} watched', { n: x.plays, h: watchedFor(x.minutes) }) }}</span></li>
          </ol>
          <p v-else class="dim">{{ t('No viewings in this time.') }}</p>
        </div>
      </div>
      <div class="panel table-wrap">
        <b>{{ t('Latest viewings') }}</b>
        <table v-if="hist.recent.length" class="table">
          <tr><th>{{ t('When') }}</th><th>{{ t('User') }}</th><th>{{ t('Title') }}</th><th></th></tr>
          <tr v-for="(e, i) in hist.recent" :key="i">
            <td class="dim nowrap">{{ fmtDateTime(e.at) }}</td><td>{{ e.user || '?' }}</td>
            <td><RouterLink v-if="e.item.title" :to="'/item/' + e.item.id">{{ what(e.item) }}</RouterLink><template v-else>{{ what(e.item) }}</template></td>
            <td class="dim nowrap">{{ e.manual ? t('marked by hand') : watchedFor(e.minutes) + (e.live ? ' · ' + t('playing now') : '') }}</td>
          </tr>
        </table>
        <p v-else class="dim">{{ t('No viewings in this time.') }}</p>
      </div>
    </template>

    <Modal v-if="stopping" :title="t('Stop playback?')" @close="stopping = null">
      <p class="dim">{{ t('“{title}” stops for {name} right away. They see your message in the Lichtspiel web app.', { title: stopping.type === 'episode' ? `${pTitle(stopping)} · ${stopping.title}` : pTitle(stopping), name: stopping.user || '?' }) }}</p>
      <textarea v-model="stopMsg" rows="3" maxlength="500" class="full" :placeholder="t('Message to {name} (optional)', { name: stopping.user || '?' })"></textarea>
      <template #actions>
        <button class="primary" :disabled="stopBusy" @click="stop">{{ t('Stop playback') }}</button>
        <button @click="stopping = null">{{ t('Cancel') }}</button>
      </template>
    </Modal>
  </div>
</template>
