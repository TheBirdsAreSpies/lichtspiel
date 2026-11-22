<script setup lang="ts">
// A library: every title as a grid of posters, loaded page by page while scrolling, with sorting, a
// filter for what is not watched yet and a search inside the library. The choice is kept per library.
// The tab "Collections" shows its collections (film series and the ones made by hand); one opens as the
// library filtered to it (?collection=…).
import { computed, nextTick, onBeforeUnmount, onMounted, ref, watch } from 'vue';
import { useRoute, useRouter } from 'vue-router';
import { get, list, picture, session, type Group, type Item } from '../api';
import { t } from '../i18n';
import PosterCard from '../components/PosterCard.vue';

const props = defineProps<{ id: string }>();
const PAGE = 60;
const SORTS: [string, string][] = [['title:asc', 'Title'], ['year:desc', 'Year (newest)'], ['year:asc', 'Year (oldest)'], ['added_at:desc', 'Recently added'],
  ['rating:desc', 'Rating'], ['last_viewed_at:desc', 'Recently watched'], ['random:asc', 'Random']];

const route = useRoute();
const router = useRouter();
const tab = computed(() => (route.query.tab === 'collections' ? 'collections' : ''));
const collection = computed(() => (typeof route.query.collection === 'string' ? route.query.collection : ''));
const section = computed(() => session.sections.find(s => String(s.id) === props.id));
const kept = (() => { try { return JSON.parse(localStorage.getItem('vue-lib:' + props.id) || '{}'); } catch { return {}; } })();
const sort = ref<string>(kept.sort || 'title:asc');
const unwatched = ref<boolean>(!!kept.unwatched);
const q = ref('');
const items = ref<Item[]>([]);
const total = ref<number | null>(null);
const loading = ref(false);
const error = ref('');
const sentinel = ref<HTMLElement | null>(null);
let seq = 0;

function url(start: number) {
  const p = new URLSearchParams({ sort: sort.value, start: String(start), size: String(PAGE) });
  if (unwatched.value) p.set(section.value && section.value.type === 'show' ? 'unwatched_leaves' : 'unwatched', '1');
  if (q.value.trim()) p.set('q', q.value.trim());
  if (collection.value) p.set('collection', collection.value);
  return `/library/sections/${props.id}/all?${p}`;
}

async function more() {
  if (loading.value || (total.value !== null && items.value.length >= total.value)) return;
  loading.value = true;
  const mine = seq;
  try {
    const j = await get<{ MediaContainer: { totalSize?: number } }>(url(items.value.length));
    if (mine !== seq) return;
    items.value.push(...list<Item>(j));
    total.value = j.MediaContainer.totalSize ?? items.value.length;
  } catch (e) { error.value = (e as Error).message; } finally { if (mine === seq) loading.value = false; }
  // The page is not full yet: the next part at once.
  requestAnimationFrame(() => { if (inView()) more(); });
}

function restart() {
  seq++; items.value = []; total.value = null; loading.value = false; error.value = '';
  try { localStorage.setItem('vue-lib:' + props.id, JSON.stringify({ sort: sort.value, unwatched: unwatched.value })); } catch { /* none */ }
  more();
}
let typing = 0;
watch([sort, unwatched, collection], restart);
watch(q, () => { clearTimeout(typing); typing = window.setTimeout(restart, 300); });

const inView = () => { const el = sentinel.value; return !!el && el.getBoundingClientRect().top < innerHeight + 600; };
let io: IntersectionObserver | null = null;
function watchEnd() {
  if (io) io.disconnect();
  io = new IntersectionObserver(e => { if (e.some(x => x.isIntersecting)) more(); }, { rootMargin: '600px' });
  if (sentinel.value) io.observe(sentinel.value);
}
onMounted(() => { if (tab.value) loadGroups(); else { more(); watchEnd(); } });
onBeforeUnmount(() => { if (io) io.disconnect(); });

// ---- collections
const groups = ref<Group[] | null>(null);
// "Hide watched" (kept per browser): each collection counts what is left, and fully seen ones go to the end.
const hideSeen = ref((() => { try { return localStorage.getItem('vue-colHideWatched') === '1'; } catch { return false; } })());
watch(hideSeen, v => { try { localStorage.setItem('vue-colHideWatched', v ? '1' : '0'); } catch { /* none */ } });
const left = (g: Group) => g.childCount - (g.viewedLeafCount || 0);
const shownGroups = computed(() => {
  const all = groups.value || [];
  return hideSeen.value ? [...all.filter(g => left(g) > 0), ...all.filter(g => left(g) <= 0)] : all;
});
async function loadGroups() {
  if (groups.value) return;
  try { groups.value = list<Group>(await get(`/library/sections/${props.id}/collections`)); } catch (e) { error.value = (e as Error).message; }
}
watch(tab, async v => {
  if (v) { loadGroups(); return; }
  await nextTick();
  watchEnd();
  if (!items.value.length) restart();
});
const openGroup = (g: Group) => router.push({ query: { collection: g.title } });
const unseen = (g: Group) => (left(g) > 0 ? t('{n} of {total} not seen', { n: left(g), total: g.childCount }) : t('All seen'));
</script>

<template>
  <div>
    <p v-if="!section" class="empty">{{ t('Library not found.') }}</p>
    <template v-else>
      <div v-if="collection" class="crumbs dim"><RouterLink :to="{ query: { tab: 'collections' } }">{{ section.title }} · {{ t('Collections') }}</RouterLink> ›</div>
      <div class="lib-head">
        <h1>{{ collection || section.title }} <span v-if="!tab && total !== null" class="dim count">{{ total }}</span></h1>
        <div v-if="!tab" class="lib-bar">
          <input v-model="q" type="search" :placeholder="t('Filter by title')" :aria-label="t('Filter by title')">
          <select v-model="sort" :aria-label="t('Sort')">
            <option v-for="[v, l] in SORTS" :key="v" :value="v">{{ t(l) }}</option>
          </select>
          <label class="chk"><input v-model="unwatched" type="checkbox"> {{ t('Unwatched') }}</label>
        </div>
      </div>
      <div v-if="!collection" class="tabs flat">
        <button :class="{ on: !tab }" @click="router.replace({ query: {} })">{{ t('Library') }}</button>
        <button :class="{ on: tab }" @click="router.replace({ query: { tab: 'collections' } })">{{ t('Collections') }}</button>
      </div>
      <p v-if="error" class="err">{{ error }}</p>
      <template v-if="tab">
        <label v-if="groups && groups.length" class="chk gap"><input v-model="hideSeen" type="checkbox"> {{ t('Hide watched') }}</label>
        <div v-if="groups === null" class="grid"><div v-for="n in 12" :key="n" class="card ph"><div class="poster skel"></div></div></div>
        <div v-else-if="groups.length" class="grid">
          <a v-for="g in shownGroups" :key="g.title" href="#" class="card" :class="{ done: hideSeen && left(g) <= 0 }" @click.prevent="openGroup(g)">
            <div class="poster">
              <img v-if="g.thumb" :src="picture(g.thumb, 360)" loading="lazy" alt="" @load="($event.target as HTMLElement).classList.add('in')">
              <span class="badge">{{ hideSeen ? left(g) : g.childCount }}</span>
            </div>
            <div class="t" :title="g.title">{{ g.title }}</div>
            <div class="s">{{ hideSeen ? unseen(g) : t('{n} items', { n: g.childCount }) }}</div>
          </a>
        </div>
        <p v-else class="empty">{{ t('No collections yet. Film series are added when details are downloaded; add your own under Edit details → Collections.') }}</p>
      </template>
      <template v-else>
      <div class="grid">
        <PosterCard v-for="it in items" :key="it.id" :item="it" />
        <template v-if="loading && !items.length"><div v-for="n in 12" :key="n" class="card ph"><div class="poster skel"></div></div></template>
      </div>
      <p v-if="total === 0" class="empty">{{ t('Nothing found.') }}</p>
      <div ref="sentinel" class="sentinel"></div>
      </template>
    </template>
  </div>
</template>
