<script setup lang="ts">
// Sync with a running Plex server (on the Libraries page): its address, one token per Plex account and whom it
// belongs to here, folder translations (Plex's folders → folders here), what is synced and which way, how
// often; testing the connection (it lists Plex's libraries and folders to pick the translation), syncing now,
// and the last sync's report.
import { onBeforeUnmount, onMounted, reactive, ref } from 'vue';
import { api, get, session } from '../api';
import { poke } from '../activity';
import { fmtDateTime } from '../format';
import { t } from '../i18n';
import { toast } from '../toast';
import FolderPicker from './FolderPicker.vue';

interface Account { id?: number, plex_name?: string, token_hint?: string, user_id: number }
interface Counts { watched: number, unwatched: number, resume: number, ratings: number }
interface Last {
  finished_at?: number, error?: string,
  accounts?: { plex_name?: string, user_id: number, error?: string, plex_items?: number, matched?: number, to_here?: Counts, to_plex?: Counts, matches?: number, markers?: number, not_found?: number, not_found_examples?: string[] }[],
}
interface State {
  enabled: boolean, url: string, interval_minutes: number, direction?: string, watched: boolean, resume: boolean, ratings: boolean, markers: boolean, matches: boolean,
  accounts: Account[], path_map: { from: string, to: string }[], last?: Last | null, running?: boolean,
}
interface Test { server?: string, version?: string, plex_name?: string, libraries: { title: string, type: string, folders: { plex: string, here?: string, in_a_library?: boolean, suggested?: string }[] }[] }
interface User { id: number, username: string, display_name?: string }

const props = defineProps<{ libraries: { title: string, paths: string[] }[] }>();
const st = ref<State | null>(null);
const users = ref<User[]>([]);
// The tokens typed in, one per account row (saved ones stay on the server; the fields empty again).
const rows = ref<{ id?: number, hint?: string, plex_name?: string, token: string, user_id: number }[]>([]);
const maps = ref<{ from: string, to: string }[]>([]);
const form = reactive({ enabled: false, url: '', interval_minutes: 15, direction: 'both', watched: true, resume: true, ratings: true, markers: false, matches: false });
const test = ref<Test | null>(null);
const testing = ref(false);
const testError = ref('');
const last = ref<Last | null>(null);
const running = ref(false);
const pickAt = ref(-1);
const kindName = (k: string) => ({ movie: t('Movies'), show: t('TV shows'), music: t('Music'), other: t('Other videos') }[k] || k);
const shown = (u?: User) => (u ? u.display_name || u.username : '?');
const here = () => props.libraries.flatMap(l => l.paths.map(p => ({ path: p, title: l.title })));

let gone = false;
onMounted(async () => {
  try {
    const [s, u] = await Promise.all([get<State>('/api/plex-sync'), get<{ users: User[] }>('/api/users')]);
    users.value = u.users;
    st.value = s;
    Object.assign(form, { enabled: s.enabled, url: s.url, interval_minutes: s.interval_minutes, direction: s.direction || 'both', watched: !!s.watched, resume: !!s.resume, ratings: !!s.ratings, markers: !!s.markers, matches: !!s.matches });
    rows.value = s.accounts.map(a => ({ id: a.id, hint: a.token_hint, plex_name: a.plex_name, token: '', user_id: a.user_id }));
    maps.value = s.path_map.map(m => ({ ...m }));
    last.value = s.last || null; running.value = !!s.running;
    if (s.running) wait();
  } catch { /* not for this server: the panel stays empty */ }
});
onBeforeUnmount(() => { gone = true; });

const body = () => ({
  ...form, interval_minutes: Number(form.interval_minutes) || 15,
  path_map: maps.value.map(m => ({ from: m.from.trim(), to: m.to.trim() })).filter(m => m.from && m.to),
  accounts: rows.value.map(r => ({ id: r.id || undefined, token: r.token.trim() || undefined, user_id: Number(r.user_id) })),
});
async function save() {
  const x = await api<State>('PUT', '/api/plex-sync', body());
  x.accounts.forEach((a, i) => { const r = rows.value[i]; if (r) { r.id = a.id; r.token = ''; r.hint = a.token_hint; r.plex_name = a.plex_name; } });
}
async function saveNow() { try { await save(); toast(t('Saved')); } catch (e) { toast((e as Error).message, 6000); } }
async function syncNow() {
  try { await save(); await api('POST', '/api/plex-sync/run'); running.value = true; poke(); wait(); } catch (e) { toast((e as Error).message, 6000); }
}
async function wait() {
  for (let i = 0; i < 1800; i++) {
    await new Promise(r => setTimeout(r, 2000));
    if (gone) return;
    const s = await get<State>('/api/plex-sync').catch(() => null);
    if (s && !s.running) { last.value = s.last || null; running.value = false; toast(t('Sync with Plex finished')); return; }
  }
}
async function runTest() {
  testing.value = true; testError.value = ''; test.value = null;
  const first = rows.value[0];
  try { test.value = await api<Test>('POST', '/api/plex-sync/test', { url: form.url.trim(), token: (first && first.token.trim()) || undefined, account_id: (first && first.id) || undefined }); }
  catch (e) { testError.value = (e as Error).message; }
  testing.value = false;
}
// A folder of Plex chosen in the test's list: its translation made, changed or taken away.
const mapOf = (plex: string) => maps.value.find(m => m.from.trim() === plex);
function choose(plex: string, to: string) {
  const m = mapOf(plex);
  if (!to) { if (m) maps.value.splice(maps.value.indexOf(m), 1); }
  else if (m) m.to = to; else maps.value.push({ from: plex, to });
}
function picked(p: string | null) {
  const m = maps.value[pickAt.value]; pickAt.value = -1;
  if (p && m) m.to = p;
}
const counts = (c?: Counts) => (c ? t('{w} watched, {u} unwatched, {r} resume points, {s} ratings', { w: c.watched, u: c.unwatched, r: c.resume, s: c.ratings }) : '');
const userName = (id: number) => shown(users.value.find(u => u.id === id));
</script>

<template>
  <section v-if="st" class="panel">
    <h2>{{ t('Sync with Plex') }}</h2>
    <p class="dim">{{ t('Keeps watched state, resume points and ratings the same here and on a running Plex server, by default both ways. Videos are matched by their file. When something changed on both sides, the side watched more recently wins; the first sync loses nothing (watched on either side stays watched).') }}</p>
    <div class="form">
      <label for="px-url">{{ t('Plex server') }}</label>
      <div><input id="px-url" v-model="form.url" placeholder="http://192.168.1.10:32400"><p class="dim hint">{{ t('Its address with the port, usually 32400.') }}</p></div>
      <label>{{ t('Plex accounts') }}</label>
      <div>
        <div v-for="(r, i) in rows" :key="i" class="row">
          <span class="who">{{ r.plex_name || t('Plex account') }}</span>
          <input v-model="r.token" type="password" autocomplete="off" class="token" :placeholder="r.hint ? t('Token {hint} (leave empty to keep)', { hint: r.hint }) : 'X-Plex-Token'">
          →
          <select v-model.number="r.user_id"><option v-for="u in users" :key="u.id" :value="u.id">{{ shown(u) }}</option></select>
          <button class="small" @click="rows.splice(i, 1)">{{ t('Remove') }}</button>
        </div>
        <button class="small" @click="rows.push({ token: '', user_id: session.me?.id || 0 })">{{ t('Add a Plex account') }}</button>
        <p class="dim hint">{{ t('One X-Plex-Token per person. Find it in Plex Web: ⋯ on any movie → Get info → View XML; the address ends in X-Plex-Token=….') }}</p>
      </div>
      <label>{{ t('Folder translation') }}</label>
      <div>
        <div v-for="(m, i) in maps" :key="i" class="row">
          <input v-model="m.from" :placeholder="t('Folder on the Plex server')">
          →
          <input v-model="m.to" :placeholder="t('Same folder here')">
          <select v-if="here().length" :title="t('Folder of a library')" value="" @change="m.to = ($event.target as HTMLSelectElement).value || m.to; ($event.target as HTMLSelectElement).value = ''">
            <option value="">{{ t('Folder of a library…') }}</option>
            <option v-for="f in here()" :key="f.path" :value="f.path">{{ f.title }} – {{ f.path }}</option>
          </select>
          <button class="small" @click="pickAt = i">{{ t('Other folder…') }}</button>
          <button class="small" @click="maps.splice(i, 1)">{{ t('Remove') }}</button>
        </div>
        <button class="small" @click="maps.push({ from: '', to: '' })">{{ t('Add a translation') }}</button>
        <p class="dim hint">{{ t('Only needed when Plex sees the videos under other paths than this server (e.g. /data/Filme there, /media/Filme here). Test connection lists the folders of Plex, choose the matching folder here for each.') }}</p>
      </div>
      <label>{{ t('Sync') }}</label>
      <div class="checks">
        <label><input v-model="form.watched" type="checkbox"> {{ t('Watched') }}</label>
        <label><input v-model="form.resume" type="checkbox"> {{ t('Resume points') }}</label>
        <label><input v-model="form.ratings" type="checkbox"> {{ t('Ratings') }}</label>
        <label><input v-model="form.markers" type="checkbox"> {{ t('Intro and credits markers (from Plex)') }}</label>
        <label><input v-model="form.matches" type="checkbox"> {{ t('Online match of titles (from Plex)') }}</label>
      </div>
      <label for="px-dir">{{ t('Direction') }}</label>
      <div>
        <select id="px-dir" v-model="form.direction">
          <option value="both">{{ t('Both ways') }}</option>
          <option value="from_plex">{{ t('Plex → here only (Plex is the master)') }}</option>
          <option value="to_plex">{{ t('Here → Plex only (this server is the master)') }}</option>
        </select>
        <p class="dim hint">{{ t('With one direction only, the master side is copied as it is: a video unwatched on the master becomes unwatched on the other side too, and nothing is ever sent the other way.') }}</p>
      </div>
      <label>{{ t('Automatically') }}</label>
      <div class="row">
        <input v-model="form.enabled" type="checkbox" :aria-label="t('Automatically')"> <span>{{ t('every') }}</span>
        <input v-model.number="form.interval_minutes" type="number" min="1" max="1440" class="num"> <span>{{ t('minutes') }}</span>
      </div>
    </div>
    <div class="actions">
      <button :disabled="testing" @click="runTest">{{ t('Test connection') }}</button>
      <button :disabled="running" @click="syncNow">{{ t('Sync now') }}</button>
      <button class="primary" @click="saveNow">{{ t('Save') }}</button>
    </div>

    <p v-if="testing" class="dim">{{ t('Asking Plex…') }}</p>
    <p v-if="testError" class="err">{{ testError }}</p>
    <div v-if="test" class="panel inner">
      <p>{{ t('Connected to {server} (Plex {version})', { server: test.server || '?', version: test.version || '?' }) }}<template v-if="test.plex_name"> · {{ t('account {name}', { name: test.plex_name }) }}</template></p>
      <div v-for="l in test.libraries" :key="l.title" class="tlib">
        <b>{{ l.title }}</b> <span class="tag">{{ kindName(l.type) }}</span>
        <div v-for="f in l.folders" :key="f.plex" class="row small-text">
          <code>{{ f.plex }}</code> →
          <select :value="mapOf(f.plex)?.to.trim() || ''" @change="choose(f.plex, ($event.target as HTMLSelectElement).value)">
            <option value="">{{ f.in_a_library && f.here === f.plex ? t('Same folder here') : t('not assigned') }}</option>
            <option v-for="h in here()" :key="h.path" :value="h.path">{{ h.path }}{{ !mapOf(f.plex) && h.path === f.suggested ? ' (' + t('suggestion') + ')' : '' }}</option>
          </select>
          <span v-if="!mapOf(f.plex) && !(f.in_a_library && f.here === f.plex) && !f.in_a_library" class="err">{{ t('not in a library here') }}</span>
        </div>
      </div>
    </div>

    <p v-if="running" class="dim">{{ t('Syncing with Plex…') }}</p>
    <div v-if="last" class="panel inner">
      <b>{{ t('Last sync') }}</b> <span class="dim">{{ fmtDateTime(last.finished_at) }}</span>
      <p v-if="last.error" class="err">{{ t(last.error) }}</p>
      <div v-for="(a, i) in last.accounts || []" :key="i" class="acc">
        <div><b>{{ a.plex_name || t('Plex account') }}</b> → {{ userName(a.user_id) }}</div>
        <div v-if="a.error" class="err">{{ t(a.error) }}</div>
        <div v-if="a.plex_items != null" class="kv">
          <div>{{ t('Found here') }}</div><div>{{ t('{n} of {all}', { n: a.matched || 0, all: a.plex_items }) }}</div>
          <div>{{ t('From Plex') }}</div><div>{{ counts(a.to_here) }}</div>
          <div>{{ t('To Plex') }}</div><div>{{ counts(a.to_plex) }}</div>
          <template v-if="a.matches"><div>{{ t('Matches from Plex') }}</div><div>{{ t('{n} items', { n: a.matches }) }}</div></template>
          <template v-if="a.markers"><div>{{ t('Markers from Plex') }}</div><div>{{ t('{n} items', { n: a.markers }) }}</div></template>
        </div>
        <details v-if="a.not_found">
          <summary class="dim">{{ t('{n} files were not found here — check the folder translation', { n: a.not_found }) }}</summary>
          <pre>{{ (a.not_found_examples || []).join('\n') }}</pre>
        </details>
      </div>
    </div>
    <FolderPicker v-if="pickAt >= 0" @done="picked" />
  </section>
</template>

<style scoped>
.row { display: flex; gap: 8px; align-items: center; margin-bottom: 6px; flex-wrap: wrap; }
.row input:not([type=checkbox]) { flex: 1; min-width: 200px; }
.row .token { min-width: 260px; }
.row .num { flex: 0 0 90px; min-width: 0; width: 90px; }
.who { min-width: 120px; }
.tlib { margin: 8px 0; }
.tlib .tag { margin: 0 0 0 4px; }
.tlib code { word-break: break-all; }
.acc { margin-top: 8px; }
details { margin-top: 6px; }
pre { white-space: pre-wrap; font-size: 12px; }
</style>
