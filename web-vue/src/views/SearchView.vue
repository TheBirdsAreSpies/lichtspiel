<script setup lang="ts">
// Search results (GET /hubs/search), grouped by kind when there are several.
import { computed, ref, watch } from 'vue';
import { useRoute } from 'vue-router';
import { get, list, type Item } from '../api';
import { t } from '../i18n';
import PosterCard from '../components/PosterCard.vue';

const KINDS: Record<string, string> = { movie: 'Movies', show: 'Shows', season: 'Seasons', episode: 'Episodes' };
const route = useRoute();
const q = computed(() => (typeof route.query.q === 'string' ? route.query.q : ''));
const results = ref<Item[] | null>(null);
const error = ref('');
let seq = 0;

watch(q, async query => {
  const mine = ++seq;
  results.value = null; error.value = '';
  if (!query) { results.value = []; return; }
  try {
    const r = list<Item>(await get('/hubs/search?query=' + encodeURIComponent(query)));
    if (mine === seq) results.value = r;
  } catch (e) { if (mine === seq) error.value = (e as Error).message; }
}, { immediate: true });

const groups = computed(() => {
  const m = new Map<string, Item[]>();
  for (const it of results.value || []) {
    const k = KINDS[it.type] ? it.type : 'other';
    if (!m.has(k)) m.set(k, []);
    m.get(k)!.push(it);
  }
  return [...m];
});
</script>

<template>
  <div>
    <h1>{{ t('Search: {q}', { q }) }}</h1>
    <p v-if="error" class="err">{{ error }}</p>
    <div v-else-if="results === null" class="grid"><div v-for="n in 12" :key="n" class="card ph"><div class="poster skel"></div></div></div>
    <p v-else-if="!results.length" class="empty">{{ t('Nothing found.') }}</p>
    <template v-else>
      <section v-for="[k, items] in groups" :key="k" class="results">
        <h2 v-if="groups.length > 1">{{ t(KINDS[k] || 'Other') }} <span class="dim count">{{ items.length }}</span></h2>
        <div class="grid"><PosterCard v-for="it in items" :key="it.id" :item="it" /></div>
      </section>
    </template>
  </div>
</template>
