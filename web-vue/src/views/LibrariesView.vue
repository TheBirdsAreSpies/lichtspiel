<script setup lang="ts">
// Libraries (owner): each library with its folders, counts and last scan, what is detected in it (credits,
// intros), how often it is scanned and in which language its details come; scanning it now, its folders and
// name, the "⋯" menu (details again, analysis, empty trash) and removing it. Below, adding a library, the
// import from the old server and the sync with a running Plex server.
import { onBeforeUnmount, onMounted, reactive, ref, watch } from 'vue';
import { api, get, list, session, type Section } from '../api';
import { activity, poke } from '../activity';
import { fmtDateTime } from '../format';
import { t } from '../i18n';
import { toast } from '../toast';
import FolderPicker from '../components/FolderPicker.vue';
import Modal from '../components/Modal.vue';
import OldServerImport from '../components/OldServerImport.vue';
import PlexSyncPanel from '../components/PlexSyncPanel.vue';

interface Lib extends Section {
  paths: string[], scanning?: boolean, scannedAt?: number, videoCount?: number, trash?: number, language?: string,
  creditsDetection?: boolean, introDetection?: boolean, scanIntervalMinutes?: number | null,
}
interface Settings { settings: { metadata_language: string }, languages: { code: string, name: string }[] }

const libs = ref<Lib[] | null>(null);
const error = ref('');
const langs = ref<{ code: string, name: string }[]>([]);
const serverLang = ref('');
const kindName = (k: string) => ({ movie: t('Movies'), show: t('TV shows'), music: t('Music'), other: t('Other videos') }[k] || k);
const intervals: [number, string][] = [[15, 'every 15 minutes'], [30, 'every 30 minutes'], [60, 'hourly'], [120, 'every 2 hours'], [360, 'every 6 hours'], [720, 'every 12 hours'], [1440, 'daily']];

// The list is read again by itself while a library is being scanned (and a few times after starting a scan,
// which begins on its own thread right after the request), so new and rescanned libraries show up.
let timer = 0;
let gone = false;
async function load(more = 0) {
  clearTimeout(timer);
  try {
    const secs = list<Lib>(await get('/library/sections'), 'Directory');
    if (gone) return;
    libs.value = secs; session.sections = secs;
    if (more > 0 || secs.some(s => s.scanning)) timer = window.setTimeout(() => load(Math.max(0, more - 1)), 2000);
  } catch (e) { error.value = (e as Error).message; }
}
onMounted(async () => {
  load();
  try {
    const st = await get<Settings>('/api/settings');
    langs.value = st.languages;
    serverLang.value = st.languages.find(l => l.code === st.settings.metadata_language)?.name || st.settings.metadata_language;
  } catch { /* the languages stay empty: "same as server" only */ }
  document.addEventListener('click', closeMenu);
});
onBeforeUnmount(() => { gone = true; clearTimeout(timer); document.removeEventListener('click', closeMenu); });
// Something in the background finished (an import made libraries, a scan ended): the counts again.
watch(() => activity.finished, () => load());

async function put(s: Lib, body: Record<string, unknown>) {
  try {
    const r = await api<{ refetching?: number }>('PUT', '/library/sections/' + s.id, body);
    if (r.refetching) { toast(t('Downloading details for {n} movies and shows again — progress is shown at the top', { n: r.refetching }), 6000); poke(); } else toast(t('Saved'));
    return true;
  } catch (e) { toast((e as Error).message, 5000); load(); return false; }
}
const value = (e: Event) => (e.target as HTMLSelectElement).value;
const checked = (e: Event) => (e.target as HTMLInputElement).checked;

async function scan(s: Lib, path?: string) {
  try {
    await api('POST', `/library/sections/${s.id}/refresh` + (path ? '?path=' + encodeURIComponent(path) : ''));
    toast(t('Scanning {name}', { name: path || s.title })); load(2); poke();
  } catch (e) { toast((e as Error).message, 5000); }
}
// A job over the whole library, answered with how many it looks at.
async function job(s: Lib, what: 'credits' | 'intros' | 'refreshMetadata' | 'analyze' | 'deepanalyze') {
  try {
    const r = await api<{ videos?: number, episodes?: number, queued?: number }>('POST', `/library/sections/${s.id}/${what}`);
    toast(what === 'credits' ? t('Checking {n} videos for credits — progress is shown at the top', { n: r.videos || 0 })
      : what === 'intros' ? t('Listening to {n} episodes for intros — progress is shown at the top', { n: r.episodes || 0 })
      : what === 'refreshMetadata' ? t('Downloading details for {n} movies and shows again — progress is shown at the top', { n: r.queued || 0 })
      : t('Analyzing {n} videos — progress is shown at the top', { n: r.videos || 0 }), 5000);
    poke();
  } catch (e) { toast((e as Error).message, 6000); }
}

// ---- the "⋯" menu of a library
const menuFor = ref<number | null>(null);
function closeMenu() { menuFor.value = null; }
function toggleMenu(s: Lib) { menuFor.value = menuFor.value === s.id ? null : s.id; }

// ---- name and folders
const editing = ref<{ s: Lib, name: string, paths: string[] } | null>(null);
// Where a picked folder goes: a new row of the edit box, one row of it, the new library, or a scan of one folder.
const picking = ref<null | { to: 'edit', at: number } | { to: 'add' } | { to: 'scan', s: Lib }>(null);
function picked(p: string | null) {
  const w = picking.value; picking.value = null;
  if (!p || !w) return;
  if (w.to === 'edit' && editing.value) { if (w.at < 0) editing.value.paths.push(p); else editing.value.paths[w.at] = p; }
  else if (w.to === 'add') add.paths = (add.paths.trim() ? add.paths.trim() + '\n' : '') + p;
  else if (w.to === 'scan') { editing.value = null; scan(w.s, p); }
}
async function saveEdit() {
  const e = editing.value; if (!e) return;
  const paths = e.paths.map(p => p.trim()).filter(Boolean);
  if (!paths.length) return toast(t('Give at least one folder in paths'));
  const body: Record<string, unknown> = { paths };
  if (e.name.trim() && e.name.trim() !== e.s.title) body.name = e.name.trim();
  if (await put(e.s, body)) { editing.value = null; load(2); poke(); }
}

// ---- empty trash, remove
const trashing = ref<Lib | null>(null);
async function emptyTrash() {
  const s = trashing.value; trashing.value = null;
  if (!s) return;
  try { const r = await api<{ removed: number }>('POST', `/library/sections/${s.id}/emptyTrash`); toast(t('{n} items taken out', { n: r.removed })); load(); }
  catch (e) { toast((e as Error).message, 5000); }
}
function askTrash(s: Lib) { if (s.trash) trashing.value = s; else toast(t('The trash of “{name}” is empty', { name: s.title })); }
const removing = ref<Lib | null>(null);
const removeTitle = (s: Lib) => t('Remove the library "{name}"? Your files stay on disk.', { name: s.title });
const introHelp = () => t('Detect show intros, showing the "Skip intro" button in the player, when enabled in server settings.');
const typed = ref('');
async function remove() {
  const s = removing.value; removing.value = null;
  if (!s) return;
  try { await api('DELETE', '/library/sections/' + s.id); toast(t('Library removed')); load(); } catch (e) { toast((e as Error).message, 5000); }
}

// ---- a new library
const add = reactive({ name: '', type: 'movie', language: '', paths: '' });
async function addLibrary() {
  try {
    await api('POST', '/library/sections', { name: add.name, type: add.type, paths: add.paths, language: add.language });
    toast(t('Library added; scanning'));
    Object.assign(add, { name: '', paths: '' });
    load(2); poke();
  } catch (e) { toast((e as Error).message, 5000); }
}
</script>

<template>
  <div>
    <h1>{{ t('Libraries') }}</h1>
    <p class="dim">
      {{ t('Movies and TV shows are kept in separate libraries. Name TV files like "Show/Season 01/Show - S01E02 - Title.mkv" and movies like "Title (Year).mkv". Every video container is picked up: ') }}
      <code>mkv mp4 avi mov ts m2ts wmv webm mpg vob flv ogv 3gp …</code>
    </p>
    <p v-if="error" class="err">{{ error }}</p>
    <div v-if="libs === null" class="skel line"></div>

    <section v-for="s in libs || []" :key="s.id" class="panel lib">
      <div class="head">
        <h2>{{ s.title }} <span class="tag">{{ kindName(s.type) }}</span><span v-if="s.scanning" class="tag smart">{{ t('scanning…') }}</span></h2>
        <div class="more">
          <button class="small" :title="t('More')" :aria-label="t('More')" aria-haspopup="menu" @click.stop="toggleMenu(s)">⋯</button>
          <div v-if="menuFor === s.id" class="menu" role="menu" @click="closeMenu">
            <button class="menu-row" role="menuitem" @click="editing = { s, name: s.title, paths: [...s.paths] }">{{ t('Edit…') }}</button>
            <button class="menu-row" role="menuitem" @click="job(s, 'refreshMetadata')">{{ t('Refresh all metadata') }}</button>
            <button class="menu-row" role="menuitem" @click="askTrash(s)">{{ s.trash ? t('Empty trash ({n})', { n: s.trash }) : t('Empty trash') }}</button>
            <button class="menu-row" role="menuitem" @click="job(s, 'analyze')">{{ t('Analyze') }}</button>
            <button class="menu-row" role="menuitem" @click="job(s, 'deepanalyze')">{{ t('Deep analysis') }}</button>
          </div>
        </div>
      </div>
      <p class="dim">
        {{ (s.type === 'show' ? t('{n} shows', { n: s.count || 0 }) + ', ' : '') + t('{n} videos · last scan {when}', { n: s.videoCount || 0, when: s.scannedAt ? fmtDateTime(s.scannedAt) : t('never') }) }}
      </p>
      <p class="dim small-text">{{ s.paths.join(' · ') }}</p>
      <div class="form">
        <label :for="'cr' + s.id" :title="t('Detect credits and allow credits skipping for items in this library when enabled in server settings.')">{{ t('Enable credits detection') }}</label>
        <div class="val"><input :id="'cr' + s.id" type="checkbox" :checked="s.creditsDetection !== false" @change="put(s, { credits_detection: checked($event) })"></div>
        <template v-if="s.type === 'show'">
          <label :for="'in' + s.id" :title="introHelp()">{{ t('Enable intro detection') }}</label>
          <div class="val"><input :id="'in' + s.id" type="checkbox" :checked="s.introDetection !== false" @change="put(s, { intro_detection: checked($event) })"></div>
        </template>
        <label :for="'iv' + s.id">{{ t('Scan interval') }}</label>
        <div>
          <select :id="'iv' + s.id" :value="s.scanIntervalMinutes ?? ''" @change="put(s, { scan_interval_minutes: value($event) === '' ? null : Number(value($event)) })">
            <option value="">{{ t('Same as server') }}</option>
            <option :value="0">{{ t('Never by itself') }}</option>
            <option v-for="[m, l] in intervals" :key="m" :value="m">{{ t(l) }}</option>
          </select>
          <p class="dim hint">{{ t('How often this library is looked at for new files. Folder watching and "Scan now" work independently.') }}</p>
        </div>
        <label :for="'la' + s.id">{{ t('Details language') }}</label>
        <div>
          <select :id="'la' + s.id" :value="s.language || ''" @change="put(s, { language: value($event) })">
            <option value="">{{ t('Same as server ({lang})', { lang: serverLang }) }}</option>
            <option v-for="l in langs" :key="l.code" :value="l.code">{{ l.name }}</option>
          </select>
          <p class="dim hint">{{ t('Language of titles, plots and episode names downloaded for this library') }}</p>
        </div>
      </div>
      <div class="actions">
        <button @click="scan(s)">{{ t('Scan now') }}</button>
        <button :title="t('Look for end credits in every video of this library now (from the chapters in the files)')" @click="job(s, 'credits')">{{ t('Detect credits') }}</button>
        <button v-if="s.type === 'show'" :title="t('Compare the sound at the start of the episodes of each season and mark the part they share as the intro')" @click="job(s, 'intros')">{{ t('Detect intros') }}</button>
        <button @click="editing = { s, name: s.title, paths: [...s.paths] }">{{ t('Folders') }}</button>
        <button class="danger" @click="removing = s; typed = ''">{{ t('Remove') }}</button>
      </div>
    </section>

    <section class="panel">
      <h2>{{ t('Add a library') }}</h2>
      <div class="form">
        <label for="l-name">{{ t('Name') }}</label><input id="l-name" v-model="add.name" :placeholder="t('Movies')">
        <label for="l-type">{{ t('Kind') }}</label>
        <select id="l-type" v-model="add.type">
          <option value="movie">{{ t('Movies') }}</option><option value="show">{{ t('TV shows') }}</option>
          <option value="music">{{ t('Music') }}</option><option value="other">{{ t('Other videos') }}</option>
        </select>
        <label for="l-lang">{{ t('Language of titles and plots') }}</label>
        <select id="l-lang" v-model="add.language">
          <option value="">{{ t('Same as server ({lang})', { lang: serverLang }) }}</option>
          <option v-for="l in langs" :key="l.code" :value="l.code">{{ l.name }}</option>
        </select>
        <label for="l-paths">{{ t('Folders') }}</label>
        <textarea id="l-paths" v-model="add.paths" rows="3" :placeholder="t('One folder per line, e.g. /media/Movies or /volume1/video/Movies')"></textarea>
      </div>
      <div class="actions">
        <button @click="picking = { to: 'add' }">{{ t('Browse folders…') }}</button>
        <button class="primary" @click="addLibrary">{{ t('Add library') }}</button>
      </div>
    </section>

    <OldServerImport />
    <PlexSyncPanel :libraries="libs || []" />

    <Modal v-if="editing" :title="t('Edit library')" wide @close="editing = null">
      <div class="form">
        <label for="e-name">{{ t('Name') }}</label><input id="e-name" v-model="editing.name">
        <label>{{ t('Kind') }}</label><div class="val">{{ kindName(editing.s.type) }}</div>
        <label>{{ t('Folders') }}</label>
        <div>
          <div v-for="(_, i) in editing.paths" :key="i" class="folder-row">
            <input v-model="editing.paths[i]" placeholder="/media/Movies">
            <button class="small" @click="picking = { to: 'edit', at: i }">{{ t('Browse…') }}</button>
            <button class="small danger" @click="editing.paths.splice(i, 1)">{{ t('Remove') }}</button>
          </div>
          <div class="actions tight">
            <button class="small" @click="picking = { to: 'edit', at: -1 }">{{ t('Add folder…') }}</button>
            <button class="small" @click="editing.paths.push('')">{{ t('Type a folder') }}</button>
          </div>
          <p class="dim hint">{{ t('The library is scanned again after saving. Removing a folder takes its videos out of the library; the files stay on disk.') }}</p>
        </div>
      </div>
      <template #actions>
        <button :title="t('Look only at one folder of this library, which is quicker than a full scan')" @click="picking = { to: 'scan', s: editing.s }">{{ t('Scan one folder…') }}</button>
        <button class="primary" @click="saveEdit">{{ t('Save') }}</button>
        <button @click="editing = null">{{ t('Cancel') }}</button>
      </template>
    </Modal>
    <FolderPicker v-if="picking" @done="picked" />
    <Modal v-if="trashing" :title="t('Empty trash')" @close="trashing = null">
      <p>{{ t('Take {n} unavailable items out of “{name}” for good? Their watched state and details go with them.', { n: trashing.trash || 0, name: trashing.title }) }}</p>
      <template #actions><button class="danger" @click="emptyTrash">{{ t('Empty trash') }}</button><button @click="trashing = null">{{ t('Cancel') }}</button></template>
    </Modal>
    <Modal v-if="removing" :title="removeTitle(removing)" @close="removing = null">
      <p class="dim">{{ t('Type “{name}” to confirm.', { name: removing.title }) }}</p>
      <input v-model="typed" class="full" autocomplete="off">
      <template #actions><button class="danger" :disabled="typed !== removing.title" @click="remove">{{ t('Remove') }}</button><button @click="removing = null">{{ t('Cancel') }}</button></template>
    </Modal>
  </div>
</template>

<style scoped>
.lib .head { display: flex; align-items: flex-start; gap: 10px; }
.lib .head h2 { flex: 1; flex-wrap: wrap; margin-bottom: 6px; }
.lib .head .tag { margin: 0; font-size: 13px; font-weight: 400; }
.more { position: relative; }
.more .menu { top: 34px; z-index: 5; }
.lib .form { margin-top: 12px; }
.folder-row { display: flex; gap: 8px; align-items: center; margin-bottom: 8px; flex-wrap: wrap; }
.folder-row input { flex: 1; min-width: 200px; max-width: none !important; }
.actions.tight { margin-top: 0; }
</style>
