<script setup lang="ts">
// What to do with the titles selected in a grid, as a bar at the bottom (as in the classic app): play them,
// the watchlist, watched or not (with "Undo"), a playlist, edit (one title), and under "⋯" shuffle, merge,
// details again, analysis and selecting all.
import { onMounted, ref } from 'vue';
import { useRouter } from 'vue-router';
import { api, get, list, session, type Item, type Playlist } from '../api';
import { poke } from '../activity';
import { isPlayable } from '../format';
import { t } from '../i18n';
import { addToQueue, playAll, smartShuffle } from '../queue';
import { toast } from '../toast';
import AskDialog from './AskDialog.vue';
import EditDetails from './EditDetails.vue';
import BulkEdit from './BulkEdit.vue';
import Modal from './Modal.vue';

const props = defineProps<{ items: Item[] }>();
const emit = defineEmits<{ clear: [], all: [], changed: [ids: number[]] }>();
const router = useRouter();
const pls = ref<Playlist[]>([]);
const asking = ref(false);
const editing = ref<number | null>(null);
const bulk = ref(false);
const merging = ref(false);
const mainAt = ref(0); // the title that stays with its details
const more = ref(false);
const ids = () => props.items.map(i => i.id);
onMounted(async () => { try { pls.value = list<Playlist>(await get('/playlists?kind=manual&owner=me&sort=title')); } catch { /* the menu has only "New" */ } });

async function run(what: () => Promise<void>) { more.value = false; try { await what(); } catch (e) { toast((e as Error).message, 6000); } }
// Everything playable in the selected titles (the episodes of a show …), in order.
async function leaves() {
  const out: Item[] = [];
  for (const it of props.items) {
    if (isPlayable(it)) { if (!it.missing) out.push(it); continue; }
    out.push(...list<Item>(await get(`/library/metadata/${it.id}/allLeaves`).catch(() => null)).filter(e => !e.missing));
  }
  return out;
}
const play = (shuffle: boolean) => run(async () => {
  const all = await leaves();
  if (!all.length) { toast(t('Nothing to add')); return; }
  playAll(router, shuffle ? smartShuffle(all) : all);
});
const watchlist = () => run(async () => {
  const r = await api<{ changed?: number }>('POST', '/api/watchlist', { ids: ids(), watchlist: true });
  toast(t('{n} added to your watchlist', { n: r.changed ?? props.items.length })); emit('clear');
});
// Watched or not; "Undo" puts back the titles that changed (when none was half watched, which it cannot restore).
const seen = (it: Item) => (it.leafCount ? (it.viewedLeafCount || 0) >= it.leafCount : (it.viewCount || 0) > 0);
const mark = (watched: boolean) => run(async () => {
  const list = props.items.slice();
  const clean = list.every(it => !it.viewOffset && (!it.leafCount || !it.viewedLeafCount || it.viewedLeafCount === it.leafCount));
  const changed = list.filter(it => seen(it) !== watched).map(it => it.id);
  await api('POST', `/:/${watched ? 'scrobble' : 'unscrobble'}?key=${list.map(it => it.id).join(',')}`);
  emit('changed', list.map(it => it.id)); emit('clear');
  const msg = list.length === 1 ? (watched ? t('Marked as watched') : t('Marked as unwatched')) : watched ? t('{n} marked as watched', { n: list.length }) : t('{n} marked as unwatched', { n: list.length });
  toast(msg, 4000, clean && changed.length ? { label: t('Undo'), run: async () => {
    await api('POST', `/:/${watched ? 'unscrobble' : 'scrobble'}?key=${changed.join(',')}`);
    emit('changed', changed);
  } } : undefined);
});
async function toPlaylist(e: Event) {
  const sel = e.target as HTMLSelectElement, v = sel.value; sel.value = '';
  if (!v) return;
  if (v === 'new') { asking.value = true; return; }
  await run(async () => { const r = await api<{ added?: number }>('POST', `/playlists/${v}/items`, { ids: ids() }); toast(r.added ? t('Added to playlist') : t('Already in that playlist')); });
}
async function newPlaylist(name: string | null) {
  asking.value = false;
  if (name) await run(async () => { await api('POST', '/playlists', { title: name, kind: 'manual', ids: ids() }); toast(t('Added to {name}', { name })); });
}
const each = (path: (it: Item) => string, msg: (n: number) => string) => run(async () => {
  for (const it of props.items) await api('POST', path(it));
  toast(msg(props.items.length), 5000); poke();
});
const canMerge = () => props.items.length > 1 && props.items.every(x => x.type === props.items[0].type && (x.type === 'movie' || x.type === 'episode'));
const merge = () => run(async () => {
  merging.value = false;
  const all = ids(), keep = all[mainAt.value] ?? all[0];
  const r = await api<{ merged: number }>('POST', '/api/merge', { ids: [keep, ...all.filter(x => x !== keep)] });
  toast(t('{n} merged into one', { n: r.merged + 1 })); emit('changed', []); emit('clear');
});
</script>

<template>
  <div class="pickbar" role="toolbar">
    <b>{{ items.length === 1 ? t('1 selected') : t('{n} selected', { n: items.length }) }}</b>
    <button class="small" @click="emit('clear')">{{ t('Clear selection') }}</button>
    <button class="small primary" @click="play(false)">{{ t('▶ Play') }}</button>
    <button class="small" @click="watchlist">{{ t('+ Watchlist') }}</button>
    <button class="small" @click="mark(true)">{{ t('Mark watched') }}</button>
    <button class="small" @click="mark(false)">{{ t('Mark unwatched') }}</button>
    <select class="small" :aria-label="t('+ Add to playlist')" @change="toPlaylist">
      <option value="">{{ t('+ Add to playlist') }}</option>
      <option v-for="p in pls" :key="p.id" :value="p.id">{{ p.title }}</option>
      <option value="new">{{ t('New playlist…') }}</option>
    </select>
    <button v-if="session.me?.admin" class="small" @click="items.length === 1 ? (editing = items[0].id) : (bulk = true)">{{ t('Edit') }}</button>
    <span class="menu-wrap">
      <button class="small" :title="t('More')" :aria-label="t('More')" :aria-expanded="more" @click="more = !more">⋯</button>
      <div v-if="more" class="menu pick-menu" role="menu">
        <button class="menu-row" role="menuitem" @click="play(true)">{{ t('Shuffle') }}</button>
        <button class="menu-row" role="menuitem" @click="run(async () => { for (const it of items) await addToQueue(it); })">{{ t('Add to queue') }}</button>
        <template v-if="session.me?.admin">
          <hr>
          <button v-if="canMerge()" class="menu-row" role="menuitem" @click="more = false; mainAt = 0; merging = true">{{ t('Merge into one…') }}</button>
          <button class="menu-row" role="menuitem" @click="each(it => `/library/metadata/${it.id}/refresh`, n => t('Downloading details for {n} titles', { n }))">{{ t('Refresh metadata') }}</button>
          <button class="menu-row" role="menuitem" @click="each(it => `/library/metadata/${it.id}/analyze`, () => t('Analyzing — progress is shown at the top'))">{{ t('Analyze') }}</button>
        </template>
        <hr>
        <button class="menu-row" role="menuitem" @click="more = false; emit('all')">{{ t('Select all') }}</button>
      </div>
    </span>
    <AskDialog v-if="asking" :title="t('Name of the new playlist')" :ok="t('Add')" @done="newPlaylist" />
    <EditDetails v-if="editing" :id="editing" @close="editing = null" @saved="emit('changed', ids()); editing = null" />
    <BulkEdit v-if="bulk" :ids="ids()" @close="bulk = false" @saved="emit('changed', ids()); emit('clear')" />
    <Modal v-if="merging" :title="t('Merge into one')" @close="merging = false">
      <p class="dim">{{ t('The one you pick stays with its details. The others are left out of the lists and offered on its page as other versions to play. Taking them apart again is possible.') }}</p>
      <label v-for="(it, i) in items" :key="it.id" class="chk merge-row"><input v-model="mainAt" type="radio" name="mainitem" :value="i"> {{ it.title }}<span v-if="it.year" class="dim"> ({{ it.year }})</span></label>
      <template #actions><button class="primary" @click="merge">{{ t('Merge') }}</button><button @click="merging = false">{{ t('Cancel') }}</button></template>
    </Modal>
  </div>
</template>
