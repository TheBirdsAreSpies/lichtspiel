<script setup lang="ts">
// "Add to …": one of your own playlists, a new one, and for an admin a collection of the title's library
// (a collection is a name in the title's details, so adding one changes its details).
import { computed, onMounted, ref } from 'vue';
import { api, get, list, session, type Group, type Item, type Playlist } from '../api';
import { t } from '../i18n';
import AskDialog from './AskDialog.vue';
import Modal from './Modal.vue';

const props = defineProps<{ item: Item }>();
const emit = defineEmits<{ close: [], changed: [] }>();
type Row = { name: string, kind: string, run: () => Promise<string> };
const rows = ref<Row[] | null>(null);
const q = ref('');
const error = ref('');
const done = ref('');
const asking = ref<'' | 'playlist' | 'collection'>('');
const LIMIT = 40;
const canCol = computed(() => !!session.me?.admin && ['movie', 'show'].includes(props.item.type) && !!props.item.librarySectionID);
let mine: string[] = [];

async function setCols(names: string[]) {
  await api('PUT', '/library/metadata/' + props.item.id, { collections: names });
  mine = names;
  emit('changed');
}

onMounted(async () => {
  try {
    const [pls, cols, full] = await Promise.all([
      get('/playlists?kind=manual&owner=me&sort=title').then(j => list<Playlist>(j)),
      canCol.value ? get(`/library/sections/${props.item.librarySectionID}/collections`).then(j => list<Group>(j).map(c => c.title)) : Promise.resolve([] as string[]),
      canCol.value ? get('/library/metadata/' + props.item.id).then(j => list<Item>(j)[0]) : Promise.resolve(null),
    ]);
    mine = (full && full.extra && full.extra.collections) || [];
    rows.value = [
      ...pls.map(p => ({ name: p.title, kind: t('Playlist'), run: async () => {
        const r = await api<{ added?: number }>('POST', `/playlists/${p.id}/items`, { ids: [props.item.id] });
        return r && r.added ? t('Added to {name}', { name: p.title }) : t('Already in that playlist');
      } })),
      ...cols.filter(c => !mine.includes(c)).map(c => ({ name: c, kind: t('Collection'), run: async () => {
        await setCols([...mine, c]);
        return t('Added to {name}', { name: c });
      } })),
    ];
  } catch (e) { error.value = (e as Error).message; }
});

const hits = computed(() => {
  const w = q.value.trim().toLowerCase();
  return (rows.value || []).filter(r => !w || r.name.toLowerCase().includes(w));
});
async function run(r: Row) {
  try { done.value = await r.run(); setTimeout(() => emit('close'), 900); } catch (e) { error.value = (e as Error).message; }
}
async function make(name: string | null) {
  const what = asking.value;
  asking.value = '';
  if (!name) return;
  try {
    if (what === 'playlist') await api('POST', '/playlists', { title: name, kind: 'manual', ids: [props.item.id] });
    else await setCols([...mine, name]);
    done.value = t('Added to {name}', { name });
    setTimeout(() => emit('close'), 900);
  } catch (e) { error.value = (e as Error).message; }
}
</script>

<template>
  <Modal :title="t('Add to …')" @close="emit('close')">
    <p v-if="done" class="ok-msg">{{ done }}</p>
    <template v-else>
      <input v-model="q" type="search" class="full" :placeholder="t('Search')" :aria-label="t('Search')">
      <div class="pick-list">
        <div v-if="rows === null" class="skel line"></div>
        <button v-for="r in hits.slice(0, LIMIT)" :key="r.kind + r.name" class="pick" @click="run(r)"><span class="grow">{{ r.name }}</span><span class="dim">{{ r.kind }}</span></button>
        <div v-if="hits.length > LIMIT" class="dim pad">{{ t('{n} more — type to narrow down', { n: hits.length - LIMIT }) }}</div>
        <div v-if="rows !== null && !hits.length" class="dim pad">{{ t('Nothing found.') }}</div>
      </div>
    </template>
    <p v-if="error" class="err">{{ error }}</p>
    <template #actions>
      <button @click="asking = 'playlist'">{{ t('New playlist…') }}</button>
      <button v-if="canCol" @click="asking = 'collection'">{{ t('New collection…') }}</button>
      <button @click="emit('close')">{{ t('Close') }}</button>
    </template>
  </Modal>
  <AskDialog v-if="asking" :title="asking === 'playlist' ? t('Name of the new playlist') : t('Name of the new collection')" :ok="t('Add')" @done="make" />
</template>
