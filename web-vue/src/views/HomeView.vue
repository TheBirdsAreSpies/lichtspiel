<script setup lang="ts">
// Home: a banner for what to go on with, then the server's rows (GET /hubs): continue watching, the
// watchlist, coming soon (from its own address: /hubs is also read by Plex apps), recently added …
import { computed, onMounted, ref } from 'vue';
import { get, list, pref, savePref, session, type Hub, type Item } from '../api';
import Modal from '../components/Modal.vue';
import ShortDialog from '../components/ShortDialog.vue';
import { loadComing, type Coming } from '../coming';
import { t } from '../i18n';
import ComingRow from '../components/ComingRow.vue';
import PosterRow from '../components/PosterRow.vue';
import SpotBanner, { type Spot } from '../components/SpotBanner.vue';

const hubs = ref<Hub[] | null>(null);
const error = ref('');
const coming = ref<Coming[]>([]);

onMounted(async () => {
  loadComing(30).then(r => { coming.value = r.entries; }).catch(() => {});
  try { hubs.value = list<Hub>(await get('/hubs'), 'Hub'); } catch (e) { error.value = (e as Error).message; }
});
// The page as each person wants it ("Customize home", kept with the account as in the classic app,
// "homeLayout"): which rows in which order, and the banner or not. Rows not known yet come at the end.
interface Layout { order: string[], hidden: string[], banner: boolean }
function readLayout(): Layout {
  let v: Partial<Layout> = {}; try { v = JSON.parse(pref('homeLayout', '{}')) || {}; } catch { /* none */ }
  return { order: Array.isArray(v.order) ? v.order : [], hidden: Array.isArray(v.hidden) ? v.hidden : [], banner: v.banner !== false };
}
const layout = ref(readLayout());
// The rows: the server's, with coming soon ("upcoming") after continue watching and the watchlist.
interface RowOf { id: string, title: string, hub?: Hub }
const allRows = computed<RowOf[]>(() => {
  const all: RowOf[] = (hubs.value || []).map(h => ({ id: h.hubIdentifier, title: t(h.title), hub: h }));
  if (coming.value.length) {
    const at = all.findIndex(x => !['continueWatching', 'watchlist'].includes(x.id));
    all.splice(at < 0 ? all.length : at, 0, { id: 'upcoming', title: t('Coming Soon') });
  }
  const lay = layout.value, pos = (id: string) => { const i = lay.order.indexOf(id); return i < 0 ? lay.order.length : i; };
  return all.map((x, i) => [x, i] as const).sort((a, b) => pos(a[0].id) - pos(b[0].id) || a[1] - b[1]).map(x => x[0]);
});
const rows = computed(() => allRows.value.filter(x => !layout.value.hidden.includes(x.id)));
// "Something short" sits on the first row it picks from (what you are watching, or the watchlist).
const shortOn = computed(() => (rows.value.find(x => x.id === 'continueWatching' || x.id === 'watchlist') || { id: '' }).id);
const short = ref(false);

// ---- customizing
const editing = ref<{ rows: { id: string, title: string, on: boolean }[], banner: boolean } | null>(null);
function startEdit() { editing.value = { rows: allRows.value.map(x => ({ id: x.id, title: x.title, on: !layout.value.hidden.includes(x.id) })), banner: layout.value.banner }; }
function move(i: number, d: number) { const l = editing.value!.rows; [l[i], l[i + d]] = [l[i + d], l[i]]; }
function saveLayout(reset = false) {
  const e = editing.value; editing.value = null;
  const v = reset || !e ? {} : { order: e.rows.map(x => x.id), hidden: e.rows.filter(x => !x.on).map(x => x.id), banner: e.banner };
  savePref('homeLayout', JSON.stringify(v)).catch(() => {});
  layout.value = readLayout();
}

// The banner: where you left off first, then what is new; one title per show; at most five (as in the classic app).
const spots = computed<Spot[]>(() => {
  if (!layout.value.banner) return [];
  const all = hubs.value || [];
  const of = (id: string) => ((all.find(h => h.hubIdentifier === id) || {}).Metadata || []) as Item[];
  const pool = [...of('continueWatching').slice(0, 2), ...of('recentlyAddedMovies').slice(0, 4), ...of('recentlyAddedEpisodes').slice(0, 3)];
  // Nothing new and nothing started: whatever the other rows hold.
  if (!pool.length) for (const h of all) pool.push(...((h.Metadata || []) as Item[]).slice(0, 3));
  const seen = new Set<string>(), out: Spot[] = [];
  for (const it of pool) {
    if (!it || it.missing || !['movie', 'show', 'season', 'episode'].includes(it.type)) continue;
    const key = String(it.grandparentRatingKey || (it.type === 'season' ? it.parentRatingKey : null) || it.id);
    if (seen.has(key)) continue;
    seen.add(key);
    const hub = all.find(h => (h.Metadata || []).includes(it));
    out.push({ it, label: hub ? t(hub.title) : '' });
    if (out.length >= 5) break;
  }
  return out;
});
const wideRow = (h: Hub) => h.hubIdentifier === 'continueWatching';
</script>

<template>
  <div>
    <p v-if="error" class="err">{{ error }}</p>
    <template v-else-if="hubs === null">
      <div class="spot skel"></div>
      <div v-for="n in 2" :key="n" class="row"><div class="skel line"></div><div class="strip"><div v-for="m in 8" :key="m" class="card ph"><div class="poster skel"></div></div></div></div>
    </template>
    <template v-else>
      <div v-if="!session.sections.length" class="empty">
        <h1>{{ t('Welcome') }}</h1>
        <p>{{ session.me?.admin ? t('No libraries yet. Add your movie and TV show folders to get started.') : t('The owner has not shared any libraries with you yet.') }}</p>
        <RouterLink v-if="session.me?.admin" to="/libraries" class="btn primary">{{ t('Add a library') }}</RouterLink>
      </div>
      <SpotBanner v-if="spots.length" :spots="spots" />
      <h1 v-if="!spots.length && session.sections.length">{{ t('Home') }}</h1>
      <template v-for="x in rows" :key="x.id">
        <ComingRow v-if="!x.hub" :entries="coming" />
        <PosterRow v-else :title="x.title" :items="x.hub.Metadata || []" :wide="wideRow(x.hub)" :cont="x.id === 'continueWatching'">
          <template v-if="x.id === shortOn" #head><button class="small h2btn" @click="short = true">{{ t('⏱ Something short') }}</button></template>
        </PosterRow>
      </template>
      <p v-if="hubs.length && !rows.length" class="empty">{{ t('All rows are hidden.') }}</p>
      <div v-if="allRows.length" class="home-edit"><button class="small" @click="startEdit">{{ t('Customize home') }}</button></div>
      <ShortDialog v-if="short" @close="short = false" />
      <Modal v-if="editing" :title="t('Customize home')" @close="editing = null">
        <label class="chk gap"><input v-model="editing.banner" type="checkbox"> {{ t('Banner at the top') }}</label>
        <div class="dim">{{ t('Rows') }}</div>
        <div class="home-rows">
          <div v-for="(x, i) in editing.rows" :key="x.id" class="li">
            <label class="grow"><input v-model="x.on" type="checkbox"> {{ x.title }}</label>
            <button class="small" :title="t('Move up')" :aria-label="t('Move up')" :disabled="i === 0" @click="move(i, -1)">▲</button>
            <button class="small" :title="t('Move down')" :aria-label="t('Move down')" :disabled="i === editing.rows.length - 1" @click="move(i, 1)">▼</button>
          </div>
        </div>
        <template #actions>
          <button class="primary" @click="saveLayout()">{{ t('Save') }}</button>
          <button @click="saveLayout(true)">{{ t('Reset') }}</button>
          <button @click="editing = null">{{ t('Cancel') }}</button>
        </template>
      </Modal>
      <p v-if="!hubs.length && session.sections.length" class="empty">{{ t('Nothing here yet. Libraries are still being scanned, or they are empty.') }}</p>
    </template>
  </div>
</template>
