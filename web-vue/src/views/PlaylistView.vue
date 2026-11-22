<script setup lang="ts">
// One playlist: play all or shuffled, rename, show on home, share, delete; its titles as a list, loaded
// page by page (a smart playlist can hold thousands of episodes), with sorting and filters. In a normal
// playlist in its own order, the titles move up and down and come out again. A smart playlist's rules are
// edited here too (SmartEditor).
import { computed, nextTick, onBeforeUnmount, onMounted, ref, watch } from 'vue';
import { useRoute, useRouter } from 'vue-router';
import { api, get, list, picture, type Item, type Playlist } from '../api';
import { fmtDur, fmtTime, kindName, plTitle, resumeAt } from '../format';
import { t } from '../i18n';
import { playAll, smartShuffle } from '../queue';
import AskDialog from '../components/AskDialog.vue';
import Icon from '../components/Icon.vue';
import Modal from '../components/Modal.vue';
import SmartEditor from '../components/SmartEditor.vue';

const props = defineProps<{ id: string }>();
const route = useRoute();
const router = useRouter();
const PAGE = 100;
const SORTS: [string, string][] = [['title', 'Title'], ['year:desc', 'Year (newest)'], ['year', 'Year (oldest)'], ['added_at:desc', 'Recently added'],
  ['rating:desc', 'Rating'], ['duration:desc', 'Length'], ['last_viewed_at:desc', 'Recently watched'], ['view_count:desc', 'Most watched'],
  ['random', 'Random'], ['episode_order', 'Show, season, episode']];

const str = (v: unknown) => (typeof v === 'string' ? v : '');
const sort = computed(() => str(route.query.sort));
const type = computed(() => str(route.query.type));
const watched = computed(() => str(route.query.watched));
const q = ref(str(route.query.q));
const pl = ref<Playlist | null>(null);
const items = ref<Item[]>([]);
const total = ref<number | null>(null);
const loading = ref(false);
const error = ref('');
const renaming = ref(false);
const deleting = ref(false);
const sentinel = ref<HTMLElement | null>(null);
let seq = 0;

const manual = computed(() => !!pl.value && pl.value.kind === 'manual');
const editable = computed(() => !!pl.value && !!pl.value.editable);
// Moving only makes sense in the playlist's own order with nothing left out.
const reorder = computed(() => manual.value && editable.value && !sort.value && !type.value && !watched.value && !str(route.query.q));

function go(change: Record<string, string>) {
  const next: Record<string, string> = { sort: sort.value, type: type.value, watched: watched.value, q: q.value.trim(), ...change };
  router.replace({ query: Object.fromEntries(Object.entries(next).filter(([, v]) => v)) });
}
let typing = 0;
watch(q, () => { clearTimeout(typing); typing = window.setTimeout(() => go({}), 300); });

function itemsUrl(start?: number) {
  const p = new URLSearchParams();
  if (sort.value) p.set('sort', sort.value);
  if (type.value) p.set('kind', type.value);
  if (watched.value) p.set('watched', watched.value === 'watched' ? 'true' : 'false');
  const w = str(route.query.q);
  if (w) p.set('q', w);
  if (start !== undefined) { p.set('start', String(start)); p.set('size', String(PAGE)); }
  return `/playlists/${props.id}/items?${p}`;
}
async function loadPlaylist() {
  try { pl.value = list<Playlist>(await get('/playlists/' + props.id))[0] || null; if (!pl.value) error.value = t('Not found'); } catch (e) { error.value = (e as Error).message; }
}
async function more() {
  if (loading.value || (total.value !== null && items.value.length >= total.value)) return;
  loading.value = true;
  const mine = seq;
  try {
    const j = await get<{ MediaContainer: { totalSize?: number } }>(itemsUrl(items.value.length));
    if (mine !== seq) return;
    items.value.push(...list<Item>(j));
    total.value = j.MediaContainer.totalSize ?? items.value.length;
  } catch (e) { error.value = (e as Error).message; } finally { if (mine === seq) loading.value = false; }
  requestAnimationFrame(() => { if (inView()) more(); });
}
function restart() { seq++; items.value = []; total.value = null; loading.value = false; more(); }
watch(() => route.query, restart);
const reload = () => { loadPlaylist(); restart(); };
const editingRules = ref(false);

const inView = () => { const el = sentinel.value; return !!el && el.getBoundingClientRect().top < innerHeight + 900; };
let io: IntersectionObserver | null = null;
onMounted(async () => {
  await loadPlaylist();
  await nextTick(); // the list (and its end marker) is on the page once the playlist is
  more();
  io = new IntersectionObserver(e => { if (e.some(x => x.isIntersecting)) more(); }, { rootMargin: '900px' });
  if (sentinel.value) io.observe(sentinel.value);
});
onBeforeUnmount(() => { if (io) io.disconnect(); });

// "Play all" and "Shuffle" take the whole playlist as it is filtered, not only the rows loaded so far.
async function playEvery(shuffle: boolean) {
  try {
    let all = list<Item>(await get(itemsUrl()));
    if (shuffle) all = smartShuffle(all);
    playAll(router, all);
  } catch (e) { error.value = (e as Error).message; }
}
// ▶ on a row plays on with the rows after it.
const playFrom = (i: number) => playAll(router, items.value.slice(i));

async function act(f: () => Promise<unknown>) {
  try { await f(); } catch (e) { error.value = (e as Error).message; }
}
const rename = (name: string | null) => { renaming.value = false; if (name) act(async () => { await api('PUT', '/playlists/' + props.id, { title: name }); await loadPlaylist(); }); };
const togglePin = () => act(async () => { await api('PUT', '/playlists/' + props.id, { pinned: !pl.value!.pinned }); await loadPlaylist(); });
const toggleShare = () => act(async () => { await api('PUT', '/playlists/' + props.id, { shared: !pl.value!.shared }); await loadPlaylist(); });
const copy = () => act(async () => { const r = await api<{ id: number }>('POST', `/playlists/${props.id}/copy`, {}); router.push('/playlist/' + r.id); });
const remove = () => act(async () => { deleting.value = false; await api('DELETE', '/playlists/' + props.id); router.push('/playlists'); });
const removeItem = (it: Item) => act(async () => { await api('DELETE', `/playlists/${props.id}/items/${it.id}`); reload(); });
function move(i: number, dir: number) {
  const list = items.value, target = i + dir;
  if (target < 0 || target >= list.length) return;
  const after = dir < 0 ? (target === 0 ? 0 : list[target - 1].id) : list[target].id;
  act(async () => {
    await api('PUT', `/playlists/${props.id}/items/${list[i].id}/move?after=${after}`);
    const [it] = list.splice(i, 1); list.splice(target, 0, it);
  });
}

const thumb = (it: Item) => picture(it.squareThumb || (it.type === 'episode' && it.grandparentThumb ? it.grandparentThumb : it.thumb), 120);
const rowTitle = (it: Item) => (it.type === 'episode' ? `${it.grandparentTitle} · ${t('S{n}', { n: it.parentIndex || 0 })}${t('E{n}', { n: it.index || 0 })} ${it.title}`
  : it.type === 'track' ? `${it.grandparentTitle || ''} · ${it.title}` : it.title);
const rowInfo = (it: Item) => [it.type === 'track' ? it.parentTitle : null, it.year, fmtDur(it.duration), it.viewCount ? t('✓ watched') : '',
  resumeAt(it) ? t('stopped at {time}', { time: fmtTime(resumeAt(it)) }) : ''].filter(Boolean).join(' · ');
</script>

<template>
  <div>
    <p v-if="error" class="err">{{ error }}</p>
    <template v-if="pl">
      <div class="crumbs dim"><RouterLink to="/playlists">{{ t('Playlists') }}</RouterLink> ›</div>
      <h1>{{ plTitle(pl) }} <span class="tag" :class="pl.kind">{{ kindName(pl.kind) }}</span></h1>
      <p v-if="pl.summary" class="dim">{{ pl.default ? t(pl.summary) : pl.summary }}</p>
      <div class="actions">
        <button class="primary" :disabled="!pl.leafCount" @click="playEvery(false)">{{ t('▶ Play all') }}</button>
        <button :disabled="!pl.leafCount" :title="t('Random order; the same show or artist does not come twice in a row while others are left')" @click="playEvery(true)"><Icon name="shuffle" />{{ t('Shuffle') }}</button>
        <button v-if="editable" @click="renaming = true">{{ t('Rename') }}</button>
        <button v-if="editable" @click="togglePin">{{ pl.pinned ? t('Remove from home') : t('Show on home') }}</button>
        <button v-if="editable && !pl.default" @click="toggleShare">{{ pl.shared ? t('Stop sharing') : t('Share with everyone') }}</button>
        <button v-if="pl.smart" @click="copy">{{ t('Save as normal playlist') }}</button>
        <button v-if="pl.smart && editable" @click="editingRules = true">{{ t('Edit rules') }}</button>
        <button v-if="editable && !pl.default" class="danger" @click="deleting = true">{{ t('Delete') }}</button>
      </div>
      <div class="lib-bar tools">
        <select :value="sort" :aria-label="t('Sort')" @change="go({ sort: ($event.target as HTMLSelectElement).value })">
          <option value="">{{ manual ? t('Playlist order') : t('Rule order') }}</option>
          <option v-for="[v, l] in SORTS" :key="v" :value="v">{{ t(l) }}</option>
        </select>
        <select :value="type" :aria-label="t('Kind')" @change="go({ type: ($event.target as HTMLSelectElement).value })">
          <option value="">{{ t('Movies and episodes') }}</option><option value="movie">{{ t('Movies') }}</option><option value="episode">{{ t('Episodes') }}</option>
        </select>
        <select :value="watched" :aria-label="t('Watched')" @change="go({ watched: ($event.target as HTMLSelectElement).value })">
          <option value="">{{ t('Watched or not') }}</option><option value="unwatched">{{ t('Unwatched') }}</option><option value="watched">{{ t('Watched') }}</option>
        </select>
        <input v-model="q" type="search" :placeholder="t('Filter by title')" :aria-label="t('Filter by title')">
        <span class="grow"></span>
        <span v-if="total !== null" class="dim">{{ t('{shown} of {all} items · {length}', { shown: total, all: pl.leafCount, length: fmtDur(pl.duration) }) }}</span>
      </div>
      <div class="episodes">
        <div v-for="(it, i) in items" :key="it.id" class="ep li">
          <RouterLink :to="'/item/' + it.id" class="li-thumb"><img v-if="thumb(it)" :src="thumb(it)" loading="lazy" alt=""></RouterLink>
          <div class="grow">
            <RouterLink :to="'/item/' + it.id"><b>{{ rowTitle(it) }}</b></RouterLink>
            <div class="dim">{{ rowInfo(it) }}</div>
          </div>
          <template v-if="reorder">
            <button class="iconbtn" :disabled="i === 0" :title="t('Move up')" :aria-label="t('Move up')" @click="move(i, -1)"><Icon name="up" /></button>
            <button class="iconbtn" :disabled="i === items.length - 1" :title="t('Move down')" :aria-label="t('Move down')" @click="move(i, 1)"><Icon name="down" /></button>
          </template>
          <button v-if="manual && editable" class="iconbtn" :title="t('Remove')" :aria-label="t('Remove')" @click="removeItem(it)"><Icon name="close" /></button>
          <button class="iconbtn round" :aria-label="t('Play')" @click="playFrom(i)"><Icon name="play" /></button>
        </div>
        <div v-if="loading && !items.length" class="skel line"></div>
      </div>
      <p v-if="total === 0" class="empty">{{ manual && !(type || watched || route.query.q) ? t('Empty. Open a movie or episode and use "Add to playlist".') : t('Nothing matches these rules right now.') }}</p>
      <div ref="sentinel" class="sentinel"></div>
      <AskDialog v-if="renaming" :title="t('New name')" :value="plTitle(pl)" @done="rename" />
      <SmartEditor v-if="editingRules" :pl="pl" @close="editingRules = false" @saved="reload" />
      <Modal v-if="deleting" :title="t('Delete this playlist?')" @close="deleting = false">
        <p>{{ plTitle(pl) }}</p>
        <template #actions>
          <button class="danger" @click="remove">{{ t('Delete') }}</button>
          <button @click="deleting = false">{{ t('Cancel') }}</button>
        </template>
      </Modal>
    </template>
  </div>
</template>
