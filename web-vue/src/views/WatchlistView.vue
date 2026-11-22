<script setup lang="ts">
// The watchlist (GET /api/watchlist): what you want to watch later, as posters, with a tab per kind, sorting
// and a search by title. The choice is kept in the address, so going back finds it as it was.
import { computed, ref, watch } from 'vue';
import { useRoute, useRouter } from 'vue-router';
import { get, list, type Item } from '../api';
import { t } from '../i18n';
import PosterCard from '../components/PosterCard.vue';

const route = useRoute();
const router = useRouter();
const TABS: [string, string][] = [['', 'All'], ['movie', 'Movies'], ['show', 'Shows'], ['episode', 'Episodes']];
const SORTS: [string, string][] = [['', 'Added to the watchlist (newest)'], ['oldest', 'Added to the watchlist (oldest)'], ['title', 'Title'],
  ['year', 'Year'], ['rating', 'Rating'], ['added_at', 'Added to the library']];

const str = (v: unknown) => (typeof v === 'string' ? v : '');
const type = computed(() => str(route.query.type));
const sort = computed(() => str(route.query.sort));
const q = ref(str(route.query.q));
const items = ref<Item[] | null>(null);
const error = ref('');

function go(change: Record<string, string>) {
  const next: Record<string, string> = { type: type.value, sort: sort.value, q: q.value.trim(), ...change };
  router.replace({ query: Object.fromEntries(Object.entries(next).filter(([, v]) => v)) });
}
let typing = 0;
watch(q, () => { clearTimeout(typing); typing = window.setTimeout(() => go({}), 300); });

let seq = 0;
watch(() => route.query, async () => {
  const p = new URLSearchParams();
  if (type.value) p.set('type', type.value);
  if (sort.value) p.set('sort', sort.value);
  const w = str(route.query.q);
  if (w) p.set('q', w);
  const mine = ++seq;
  error.value = '';
  try {
    const r = list<Item>(await get('/api/watchlist?' + p));
    if (mine === seq) items.value = r;
  } catch (e) { if (mine === seq) error.value = (e as Error).message; }
}, { immediate: true });
const filtered = computed(() => !!(type.value || str(route.query.q)));
</script>

<template>
  <div>
    <div class="lib-head">
      <h1>{{ t('Watchlist') }} <span v-if="items" class="dim count">{{ items.length }}</span></h1>
      <div class="lib-bar">
        <input v-model="q" type="search" :placeholder="t('Filter by title')" :aria-label="t('Filter by title')">
        <select :value="sort" :aria-label="t('Sort')" @change="go({ sort: ($event.target as HTMLSelectElement).value })">
          <option v-for="[v, l] in SORTS" :key="v" :value="v">{{ t(l) }}</option>
        </select>
      </div>
    </div>
    <p class="dim intro">{{ t('What you want to watch later. A movie or episode leaves the list once you have watched it, a show once every episode is watched.') }}</p>
    <div class="tabs flat">
      <button v-for="[v, l] in TABS" :key="v" :class="{ on: type === v }" @click="go({ type: v })">{{ t(l) }}</button>
    </div>
    <p v-if="error" class="err">{{ error }}</p>
    <div v-else-if="items === null" class="grid"><div v-for="n in 12" :key="n" class="card ph"><div class="poster skel"></div></div></div>
    <div v-else-if="items.length" class="grid"><PosterCard v-for="it in items" :key="it.id" :item="it" /></div>
    <p v-else class="empty">{{ filtered ? t('Nothing matches these filters.') : t('Your watchlist is empty. Use “Add to watchlist” on the page of a movie or show.') }}</p>
  </div>
</template>
