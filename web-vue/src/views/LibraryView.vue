<script setup lang="ts">
// A library: every title as a grid of posters, loaded page by page while scrolling, with sorting, a
// filter for what is not watched yet and a search inside the library. The choice is kept per library.
import { computed, onBeforeUnmount, onMounted, ref, watch } from 'vue';
import { get, list, session, type Item } from '../api';
import { t } from '../i18n';
import PosterCard from '../components/PosterCard.vue';

const props = defineProps<{ id: string }>();
const PAGE = 60;
const SORTS: [string, string][] = [['title:asc', 'Title'], ['year:desc', 'Year (newest)'], ['year:asc', 'Year (oldest)'], ['added_at:desc', 'Recently added'],
  ['rating:desc', 'Rating'], ['last_viewed_at:desc', 'Recently watched'], ['random:asc', 'Random']];

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
watch([sort, unwatched], restart);
watch(q, () => { clearTimeout(typing); typing = window.setTimeout(restart, 300); });

const inView = () => { const el = sentinel.value; return !!el && el.getBoundingClientRect().top < innerHeight + 600; };
let io: IntersectionObserver | null = null;
onMounted(() => {
  more();
  io = new IntersectionObserver(e => { if (e.some(x => x.isIntersecting)) more(); }, { rootMargin: '600px' });
  if (sentinel.value) io.observe(sentinel.value);
});
onBeforeUnmount(() => { if (io) io.disconnect(); });
</script>

<template>
  <div>
    <p v-if="!section" class="empty">{{ t('Library not found.') }}</p>
    <template v-else>
      <div class="lib-head">
        <h1>{{ section.title }} <span v-if="total !== null" class="dim count">{{ total }}</span></h1>
        <div class="lib-bar">
          <input v-model="q" type="search" :placeholder="t('Filter by title')" :aria-label="t('Filter by title')">
          <select v-model="sort" :aria-label="t('Sort')">
            <option v-for="[v, l] in SORTS" :key="v" :value="v">{{ t(l) }}</option>
          </select>
          <label class="chk"><input v-model="unwatched" type="checkbox"> {{ t('Unwatched') }}</label>
        </div>
      </div>
      <p v-if="error" class="err">{{ error }}</p>
      <div class="grid">
        <PosterCard v-for="it in items" :key="it.id" :item="it" />
        <template v-if="loading && !items.length"><div v-for="n in 12" :key="n" class="card ph"><div class="poster skel"></div></div></template>
      </div>
      <p v-if="total === 0" class="empty">{{ t('Nothing found.') }}</p>
      <div ref="sentinel" class="sentinel"></div>
    </template>
  </div>
</template>
