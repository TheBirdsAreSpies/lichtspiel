<script setup lang="ts">
// Statistics, for everyone: one's own viewing (the owner sees everyone's or picks one person) over the last
// month, year or everything: totals, per day, week or month, the times of day, the most watched shows,
// movies and genres, who watches how much; for the owner also the load on the server per hour of the day.
import { computed, onMounted, ref, watch } from 'vue';
import { get, picture, session } from '../api';
import { fmtDur, fmtNum, hour12 } from '../format';
import { lang, t } from '../i18n';
import BarChart, { type Bucket } from '../components/BarChart.vue';
import HeatChart from '../components/HeatChart.vue';

type Kind = 'movie' | 'episode' | 'other';
interface Ranked { id: number, title?: string, year?: number, thumb?: string, plays: number, minutes: number }
interface Stats {
  from: number, size: number, minutes: number, plays: [number, number, Kind, number][], shows: Ranked[], movies: Ranked[],
  genres: { genre: string, plays: number, minutes: number }[], users: { id: number, username: string, display_name?: string, plays: number, minutes: number }[],
}
interface Person { id: number, username: string, display_name?: string }
interface Sample { at: number, cpu?: number, streams?: number, transcodes?: number }

const RANGES: [number, string][] = [[30, '30 days'], [90, '90 days'], [365, '1 year'], [3660, 'Everything']];
const KINDS: [string, string, string][] = [['movie', 'Movies', 'var(--s1)'], ['episode', 'Episodes', 'var(--s2)'], ['other', 'Other videos', 'var(--s3)']];
const METRICS: ['plays' | 'hours', string][] = [['plays', 'Viewings'], ['hours', 'Hours']];
const admin = computed(() => !!(session.me && session.me.admin));
const days = ref(Number(localStorage.getItem('vue-statsDays')) || 365);
const metric = ref<'plays' | 'hours'>(localStorage.getItem('vue-statsMetric') === 'plays' ? 'plays' : 'hours');
const user = ref('');
const people = ref<Person[]>([]);
const r = ref<Stats | null>(null);
const error = ref('');
watch(days, v => localStorage.setItem('vue-statsDays', String(v)));
watch(metric, v => localStorage.setItem('vue-statsMetric', v));

async function load() {
  try { r.value = await get<Stats>(`/api/stats/plays?days=${days.value}${admin.value && user.value ? '&user=' + user.value : ''}`); error.value = ''; }
  catch (e) { error.value = (e as Error).message; }
}
watch([days, user], load);
onMounted(() => {
  load();
  if (admin.value) {
    get<Person[] | { users: Person[] }>('/api/users').then(x => { people.value = Array.isArray(x) ? x : x.users || []; }).catch(() => {});
    loadLoad();
  }
});

const hours = computed(() => metric.value === 'hours');
const fmt = (v: number, full?: boolean) => (hours.value ? fmtNum(v, v && v < 10 ? 1 : 0) + (full ? ' h' : '') : fmtNum(Math.round(v * 100) / 100, v % 1 ? 1 : 0));
const shownName = (u: { username: string, display_name?: string }) => u.display_name || u.username || '?';
const watchedFor = (mins: number) => fmtDur(mins * 60000) || '0 min';
const hourName = (h: number) => new Date(2024, 0, 1, h).toLocaleTimeString(lang.value, { hour: '2-digit', minute: '2-digit', hour12: hour12() });

// Per day, per week beyond 90 days, per month beyond a year (counted in this browser's time zone).
const view = computed(() => {
  const s = r.value; if (!s) return null;
  const add = (mins: number) => (hours.value ? mins / 60 : 1);
  const first = s.plays.length ? s.plays[0][0] : s.from;
  const span = Math.max(1, Math.ceil((Date.now() / 1000 - Math.max(s.from, first)) / 86400));
  const unit = span > 400 ? 'month' : span > 90 ? 'week' : 'day';
  const day0 = new Date(); day0.setHours(0, 0, 0, 0);
  const start = new Date(day0); start.setDate(start.getDate() - (Math.min(span, days.value) - 1));
  if (unit === 'week') start.setDate(start.getDate() - ((start.getDay() + 6) % 7));
  if (unit === 'month') start.setDate(1);
  const buckets: (Bucket & { from: number, to: number })[] = [];
  for (const d = new Date(start); d <= day0;) {
    const from = new Date(d);
    if (unit === 'month') d.setMonth(d.getMonth() + 1); else d.setDate(d.getDate() + (unit === 'week' ? 7 : 1));
    buckets.push({
      from: from.getTime() / 1000, to: d.getTime() / 1000, parts: { movie: 0, episode: 0, other: 0 },
      label: unit === 'month' ? from.toLocaleDateString(lang.value, { month: 'short', year: '2-digit' }) : from.toLocaleDateString(lang.value, { day: 'numeric', month: 'numeric' }),
      tip: unit === 'month' ? from.toLocaleDateString(lang.value, { month: 'long', year: 'numeric' }) : unit === 'week' ? t('Week of {date}', { date: from.toLocaleDateString(lang.value) })
        : from.toLocaleDateString(lang.value, { weekday: 'long', day: 'numeric', month: 'long', year: 'numeric' }),
    });
  }
  // Weekday by hour, and the days with any viewing.
  const grid = Array.from({ length: 7 }, () => new Array<number>(24).fill(0));
  const byDay = new Map<string, number>();
  let movies = 0, episodes = 0;
  for (const [at, , kind, mins] of s.plays) {
    const d = new Date(at * 1000);
    const b = buckets.find(x => at >= x.from && at < x.to);
    if (b) b.parts[kind] += add(mins);
    grid[(d.getDay() + 6) % 7][d.getHours()] += add(mins);
    const key = d.toDateString();
    byDay.set(key, (byDay.get(key) || 0) + add(mins));
    if (kind === 'movie') movies++; else if (kind === 'episode') episodes++;
  }
  const best = [...byDay.entries()].sort((a, b) => b[1] - a[1])[0];
  const byHour = grid[0].map((_, h) => grid.reduce((a, row) => a + row[h], 0));
  return { unit, buckets, grid, movies, episodes, daysWith: byDay.size, best, peak: byHour.indexOf(Math.max(...byHour)) };
});
const ranked = computed(() => (r.value ? [['Most watched shows', r.value.shows], ['Most watched movies', r.value.movies]] as [string, Ranked[]][] : []));
const sortBy = <T extends { plays: number, minutes: number }>(l?: T[]) => (l || []).slice().sort((a, b) => (hours.value ? b.minutes - a.minutes : b.plays - a.plays));
const amount = (x: { plays: number, minutes: number }) => (hours.value ? watchedFor(x.minutes) : t('{n}×', { n: x.plays }));

// ---- the processor per hour of the day, from the hour averages of the last 30 days (owner)
const load30 = ref<Bucket[] | null>(null);
const loadError = ref('');
async function loadLoad() {
  try {
    const x = await get<{ samples?: Sample[] }>('/api/dashboard/stats?range=30d');
    const sum = Array.from({ length: 24 }, () => ({ cpu: 0, n: 0, streams: 0, transcodes: 0, k: 0 }));
    for (const s of x.samples || []) {
      if (s.streams === undefined) continue;
      const b = sum[new Date(s.at * 1000).getHours()];
      b.k++; b.streams += s.streams || 0; b.transcodes += s.transcodes || 0;
      if (typeof s.cpu === 'number') { b.cpu += s.cpu; b.n++; }
    }
    load30.value = sum.some(b => b.k) ? sum.map((b, h) => ({
      label: String(h), parts: { cpu: b.n ? b.cpu / b.n : 0 },
      tip: `${hourName(h)} – ${hourName((h + 1) % 24)} · ` + t('{n} streams, {c} converted on average', { n: fmtNum(b.k ? b.streams / b.k : 0, 1), c: fmtNum(b.k ? b.transcodes / b.k : 0, 1) }),
    })) : [];
  } catch (e) { loadError.value = (e as Error).message; }
}
const LOAD_KEYS: [string, string, string][] = [['cpu', 'Processor', 'var(--s3)']];
const pctFmt = (v: number, full?: boolean) => fmtNum(v, v && v < 10 ? 1 : 0) + (full ? ' %' : '');
</script>

<template>
  <div class="dash">
    <div class="dash-head">
      <h1>{{ t('Statistics') }}</h1>
      <div class="hist-tools">
        <div class="seg"><button v-for="[v, l] in RANGES" :key="v" class="small" :class="{ on: days === v }" @click="days = v">{{ t(l) }}</button></div>
        <div class="seg"><button v-for="[v, l] in METRICS" :key="v" class="small" :class="{ on: metric === v }" @click="metric = v">{{ t(l) }}</button></div>
        <select v-if="people.length" v-model="user" :aria-label="t('User')"><option value="">{{ t('Everyone') }}</option><option v-for="u in people" :key="u.id" :value="String(u.id)">{{ shownName(u) }}</option></select>
      </div>
    </div>
    <p v-if="error" class="err">{{ error }}</p>
    <div v-else-if="!r || !view" class="skel line"></div>
    <template v-else>
      <div class="tiles">
        <div class="tile"><div class="dim lbl">{{ t('Watched') }}</div><div class="val">{{ watchedFor(r.minutes) }}</div><div class="dim note">{{ t('{n} viewings', { n: r.size }) }}</div></div>
        <div class="tile"><div class="dim lbl">{{ t('Movies') }}</div><div class="val">{{ fmtNum(view.movies, 0) }}</div></div>
        <div class="tile"><div class="dim lbl">{{ t('Episodes') }}</div><div class="val">{{ fmtNum(view.episodes, 0) }}</div></div>
        <div class="tile"><div class="dim lbl">{{ t('Days with viewing') }}</div><div class="val">{{ fmtNum(view.daysWith, 0) }}</div>
          <div v-if="view.best" class="dim note">{{ t('Most on {date}: {v}', { date: new Date(view.best[0]).toLocaleDateString(lang), v: fmt(view.best[1], true) }) }}</div></div>
        <div v-if="r.plays.length" class="tile"><div class="dim lbl">{{ t('Favourite time') }}</div><div class="val">{{ hourName(view.peak) }}</div>
          <div class="dim note">{{ t('{a} to {b}', { a: hourName(view.peak), b: hourName((view.peak + 1) % 24) }) }}</div></div>
      </div>
      <div class="panel">
        <div class="ch-title"><b>{{ hours ? t('Hours watched per {unit}', { unit: t(view.unit) }) : t('Viewings per {unit}', { unit: t(view.unit) }) }}</b></div>
        <BarChart :buckets="view.buckets" :keys="KINDS" :fmt="fmt" />
      </div>
      <div class="panel">
        <div class="ch-title"><b>{{ t('Times of day') }}</b><span class="dim">{{ hours ? t('Hours watched') : t('Viewings') }}</span></div>
        <HeatChart :grid="view.grid" :fmt="fmt" />
      </div>
      <div class="dash-cols">
        <div v-for="[label, items] in ranked" :key="label" class="panel"><b>{{ t(label) }}</b>
          <ol v-if="items && items.length" class="ranked">
            <li v-for="x in sortBy(items)" :key="x.id">
              <img :src="picture(x.thumb, 60)" alt="" loading="lazy">
              <div class="grow"><RouterLink :to="'/item/' + x.id">{{ x.title || t('(removed)') }}</RouterLink><div class="dim">{{ x.year || '' }}</div></div>
              <span class="dim">{{ amount(x) }}</span>
            </li>
          </ol>
          <p v-else class="dim">{{ t('No viewings in this time.') }}</p>
        </div>
      </div>
      <div class="dash-cols">
        <div class="panel"><b>{{ t('Genres') }}</b>
          <ol v-if="r.genres && r.genres.length" class="ranked">
            <li v-for="x in sortBy(r.genres)" :key="x.genre"><div class="grow">{{ x.genre }}</div><span class="dim">{{ amount(x) }}</span></li>
          </ol>
          <p v-else class="dim">{{ t('No viewings in this time.') }}</p>
        </div>
        <div v-if="admin && !user" class="panel"><b>{{ t('Who watches how much') }}</b>
          <ol v-if="r.users && r.users.length" class="ranked">
            <li v-for="x in sortBy(r.users)" :key="x.id"><div class="grow"><b>{{ shownName(x) }}</b></div><span class="dim">{{ t('{n} viewings · {h} watched', { n: x.plays, h: watchedFor(x.minutes) }) }}</span></li>
          </ol>
          <p v-else class="dim">{{ t('No viewings in this time.') }}</p>
        </div>
      </div>
    </template>
    <div v-if="admin" class="panel">
      <div class="ch-title"><b>{{ t('Load on the server by time of day') }}</b><span class="dim">{{ t('Average of the last 30 days') }}</span></div>
      <p v-if="loadError" class="dim">{{ loadError }}</p>
      <div v-else-if="load30 === null" class="skel line"></div>
      <p v-else-if="!load30.length" class="dim">{{ t('Nothing measured yet.') }}</p>
      <BarChart v-else :buckets="load30" :keys="LOAD_KEYS" :fmt="pctFmt" :height="180" />
    </div>
  </div>
</template>
