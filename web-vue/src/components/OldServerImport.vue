<script setup lang="ts">
// Import from the old server (on the Libraries page): where its data is, a look inside (its folders and where
// each is here, whose watched state goes to whom, what to bring over), the import itself, and the last report.
import { onBeforeUnmount, onMounted, reactive, ref } from 'vue';
import { api, get, session } from '../api';
import { poke } from '../activity';
import { fmtDateTime } from '../format';
import { t } from '../i18n';
import { toast } from '../toast';
import FolderPicker from './FolderPicker.vue';

interface Report {
  finished_at?: number, files?: { found_here?: number, in_old_database?: number, not_found?: number, not_found_examples?: string[] },
  libraries_created?: { name: string }[], watched?: { watched?: number, resume_points?: number, ratings?: number, accounts_skipped?: string[] },
  playlists?: { created?: string[], updated?: string[], smart_not_imported?: string[] }, credits_markers?: number, details?: number,
  matches?: number | null, match_candidates?: number, match_examples_without_id?: string[],
}
interface State { detected?: string, running?: boolean, last?: Report | null }
interface Preview {
  db: string, warnings: string[],
  libraries: { name: string, type: string, imported: boolean, folders: { old: string, suggested?: string }[] }[],
  accounts: { id: number | string, name: string, owner?: boolean, watched: number, suggested_user?: number }[],
  counts: Record<string, number>,
}
interface User { id: number, username: string, display_name?: string }

const where = ref('');
const detected = ref(false);
const last = ref<Report | null>(null);
const looking = ref(false);
const error = ref('');
const pv = ref<Preview | null>(null);
const users = ref<User[]>([]);
const maps = ref<{ from: string, to: string }[]>([]);
const accounts = ref<Record<string, number>>({});
const opt = reactive({ libraries: true, watched: true, playlists: true, markers: true, details: true });
const started = ref(false);
const pickAt = ref(-1);
const kindName = (k: string) => ({ movie: t('Movies'), show: t('TV shows'), music: t('Music'), other: t('Other videos') }[k] || k);
const shown = (u: User) => u.display_name || u.username;

let gone = false;
onMounted(async () => {
  try {
    const st = await get<State>('/api/import/old-server');
    where.value = st.detected || ''; detected.value = !!st.detected; last.value = st.last || null;
    if (st.running) waitDone();
  } catch (e) { error.value = (e as Error).message; }
});
onBeforeUnmount(() => { gone = true; });

async function look() {
  looking.value = true; error.value = ''; pv.value = null;
  try {
    const [p, u] = await Promise.all([get<Preview>('/api/import/old-server/preview?db=' + encodeURIComponent(where.value)), get<{ users: User[] }>('/api/users')]);
    users.value = u.users;
    maps.value = p.libraries.filter(l => l.imported).flatMap(l => l.folders.map(f => ({ from: f.old, to: f.suggested || f.old })));
    const acc: Record<string, number> = {};
    for (const a of p.accounts) acc[String(a.id)] = a.owner ? (session.me?.id || 0) : (a.suggested_user || 0);
    accounts.value = acc;
    started.value = false;
    pv.value = p;
  } catch (e) { error.value = (e as Error).message; }
  looking.value = false;
}
const mapFor = (old: string) => maps.value.find(m => m.from === old);

async function start() {
  if (!pv.value) return;
  const accs: Record<string, number> = {};
  for (const [id, u] of Object.entries(accounts.value)) if (u) accs[id] = u;
  const body = { db: pv.value.db, path_map: maps.value.map(m => ({ from: m.from, to: m.to.trim() })), accounts: accs, ...opt };
  try { await api('POST', '/api/import/old-server', body); started.value = true; toast(t('Import started — progress is shown at the top'), 5000); poke(); waitDone(); }
  catch (e) { toast((e as Error).message, 6000); }
}
async function waitDone() {
  for (let i = 0; i < 2400; i++) {
    await new Promise(r => setTimeout(r, 1500));
    if (gone) return;
    const s = await get<State>('/api/import/old-server').catch(() => null);
    if (s && !s.running) { last.value = s.last || null; toast(t('Import finished')); poke(); return; }
  }
}
function picked(p: string | null) {
  const m = maps.value[pickAt.value]; pickAt.value = -1;
  if (p && m) m.to = p;
}
const c = (k: string) => (pv.value ? pv.value.counts[k] || 0 : 0);
</script>

<template>
  <section class="panel">
    <h2>{{ t('Import from your old server') }}</h2>
    <p class="dim">
      {{ t('Brings over libraries, everyone’s watched state, resume points and ratings, playlists, credits markers and titles/plots from the server this one replaces. It reads a copy of its library database (') }}<code>com.plexapp.plugins.library.db</code>{{ t('); the old server keeps working. You can run it again any time — nothing is counted twice.') }}
    </p>
    <div class="form">
      <label for="imp-db">{{ t('Old server’s data') }}</label>
      <div>
        <input id="imp-db" v-model="where" class="wide" :placeholder="t('Data folder of the old server, or its library .db file')">
        <p class="dim hint">{{ detected ? t('Found automatically.') : t('Not found automatically. On a Synology with Docker, add the old server’s data folder to docker-compose.yml (see the README) — it then appears under /oldserver.') }}</p>
      </div>
    </div>
    <div class="actions"><button :disabled="looking" @click="look">{{ t('Look inside') }}</button></div>
    <p v-if="looking" class="dim">{{ t('Reading the old database (a copy; the old server is not changed)…') }}</p>
    <p v-if="error" class="err">{{ error }}</p>

    <template v-if="pv">
      <p>{{ t('Found: {movies} movies, {shows} shows, {episodes} episodes, {watched} watched, {resume} resume points, {ratings} ratings, {playlists} playlists (+{smart} smart), {markers} credits markers.',
        { movies: c('movies'), shows: c('shows'), episodes: c('episodes'), watched: c('watched_states'), resume: c('resume_points'), ratings: c('ratings'), playlists: c('playlists'), smart: c('smart_playlists'), markers: c('credits_markers') }) }}</p>
      <h3>{{ t('Folders') }}</h3>
      <p class="dim">{{ t('Where each folder of the old server is on this server. In Docker your video share is usually under /media.') }}</p>
      <div v-for="l in pv.libraries" :key="l.name" class="lib">
        <b>{{ l.name }}</b> <span class="tag">{{ kindName(l.type) }}</span>
        <span v-if="!l.imported" class="dim"> — {{ t('not imported (only movies and TV shows)') }}</span>
        <template v-else>
          <div v-for="f in l.folders" :key="f.old" class="map">
            <code>{{ f.old }}</code> →
            <input v-if="mapFor(f.old)" v-model="mapFor(f.old)!.to">
            <button class="small" @click="pickAt = maps.findIndex(m => m.from === f.old)">{{ t('Browse…') }}</button>
          </div>
        </template>
      </div>
      <h3>{{ t('People') }}</h3>
      <p class="dim">{{ t('Whose watched state, ratings and playlists go to which user here. Invite the others first to bring theirs over, then import again.') }}</p>
      <table class="table">
        <tr v-for="a in pv.accounts" :key="a.id">
          <td>{{ a.name }} <span v-if="a.owner" class="tag">{{ t('owner') }}</span></td>
          <td class="dim">{{ t('{n} watched', { n: a.watched }) }}</td>
          <td>→ <select v-model.number="accounts[String(a.id)]"><option :value="0">{{ t('skip') }}</option><option v-for="u in users" :key="u.id" :value="u.id">{{ shown(u) }}</option></select></td>
        </tr>
      </table>
      <h3>{{ t('What to bring over') }}</h3>
      <div class="checks">
        <label><input v-model="opt.libraries" type="checkbox"> {{ t('Libraries (added and scanned)') }}</label>
        <label><input v-model="opt.watched" type="checkbox"> {{ t('Watched, resume points and ratings') }}</label>
        <label><input v-model="opt.playlists" type="checkbox"> {{ t('Playlists') }}</label>
        <label><input v-model="opt.markers" type="checkbox"> {{ t('Credits markers') }}</label>
        <label><input v-model="opt.details" type="checkbox"> {{ t('Titles, plots and genres (kept; online details add posters and cast)') }}</label>
      </div>
      <div class="actions"><button class="primary" :disabled="started" @click="start">{{ t('Import') }}</button></div>
      <p v-if="pv.warnings.length" class="dim small-text">{{ t('Notes: ') + pv.warnings.map(w => t(w)).join('; ') }}</p>
    </template>

    <div v-if="last" class="panel inner">
      <b>{{ t('Last import') }}</b> <span class="dim">{{ fmtDateTime(last.finished_at) }}</span>
      <div class="kv">
        <div>{{ t('Files found here') }}</div><div>{{ t('{n} of {all}', { n: last.files?.found_here || 0, all: last.files?.in_old_database || 0 }) }}</div>
        <div>{{ t('Libraries added') }}</div><div>{{ (last.libraries_created || []).map(l => l.name).join(', ') || t('none (existing ones used)') }}</div>
        <div>{{ t('Watched / resume points / ratings') }}</div><div>{{ last.watched?.watched || 0 }} / {{ last.watched?.resume_points || 0 }} / {{ last.watched?.ratings || 0 }}</div>
        <div>{{ t('Playlists') }}</div><div>{{ [...(last.playlists?.created || []), ...(last.playlists?.updated || [])].join(', ') || t('none') }}</div>
        <div>{{ t('Credits markers') }}</div><div>{{ last.credits_markers || 0 }}</div>
        <div>{{ t('Titles and plots') }}</div><div>{{ t('{n} items', { n: last.details || 0 }) }}</div>
        <template v-if="last.matches != null"><div>{{ t('Online match from Plex') }}</div><div>{{ t('{n} of {all}', { n: last.matches, all: last.match_candidates || last.matches }) }}</div></template>
        <template v-if="(last.playlists?.smart_not_imported || []).length"><div>{{ t('Not imported') }}</div><div>{{ t('smart playlists (their rules work differently here): ') + last.playlists!.smart_not_imported!.join(', ') }}</div></template>
        <template v-if="(last.watched?.accounts_skipped || []).length"><div>{{ t('Accounts skipped') }}</div><div>{{ last.watched!.accounts_skipped!.join(', ') }}</div></template>
      </div>
      <details v-if="(last.match_examples_without_id || []).length">
        <summary class="dim">{{ t('Titles without an online number in the Plex database (examples)') }}</summary>
        <pre>{{ last.match_examples_without_id!.join('\n') }}</pre>
      </details>
      <details v-if="last.files?.not_found">
        <summary class="dim">{{ t('{n} files were not found here — check the folder translation', { n: last.files.not_found }) }}</summary>
        <pre>{{ (last.files.not_found_examples || []).join('\n') }}</pre>
      </details>
    </div>
    <FolderPicker v-if="pickAt >= 0" @done="picked" />
  </section>
</template>

<style scoped>
.form input.wide { max-width: 620px; }
.lib { margin: 10px 0; }
.lib .tag { margin: 0 0 0 4px; }
.map { display: flex; gap: 8px; align-items: center; margin-top: 6px; flex-wrap: wrap; }
.map code { flex: 1; min-width: 220px; word-break: break-all; }
.map input { flex: 1; min-width: 220px; }
details { margin-top: 8px; }
pre { white-space: pre-wrap; font-size: 12px; }
</style>
