<script setup lang="ts">
// Home: a banner for what to go on with, then the server's rows (GET /hubs): continue watching, the
// watchlist, coming soon (from its own address: /hubs is also read by Plex apps), recently added …
import { computed, onMounted, ref } from 'vue';
import { useRouter } from 'vue-router';
import { get, list, picture, session, type Hub, type Item } from '../api';
import { loadComing, type Coming } from '../coming';
import { resumeAt } from '../format';
import { t } from '../i18n';
import ComingRow from '../components/ComingRow.vue';
import PosterRow from '../components/PosterRow.vue';

const router = useRouter();
const hubs = ref<Hub[] | null>(null);
const error = ref('');
const coming = ref<Coming[]>([]);

onMounted(async () => {
  loadComing(30).then(r => { coming.value = r.entries; }).catch(() => {});
  try { hubs.value = list<Hub>(await get('/hubs'), 'Hub'); } catch (e) { error.value = (e as Error).message; }
});
// Coming soon joins the rows after continue watching and the watchlist.
const comingAt = computed(() => {
  const all = hubs.value || [];
  const at = all.findIndex(h => !['continueWatching', 'watchlist'].includes(h.hubIdentifier));
  return at < 0 ? all.length : at;
});

// The banner: the first title to go on with, else the newest one added.
const spot = computed<Item | null>(() => {
  const all = hubs.value || [];
  const pick = (id: string) => all.find(h => h.hubIdentifier === id && (h.Metadata || []).length);
  const hub = pick('continueWatching') || all.find(h => (h.Metadata || []).length);
  return hub ? (hub.Metadata as Item[])[0] : null;
});
const spotArt = computed(() => {
  const it = spot.value;
  if (!it) return '';
  const id = it.type === 'episode' ? it.grandparentRatingKey : it.id;
  return picture(`/library/metadata/${id}/art`);
});
const spotTitle = computed(() => (spot.value ? (spot.value.type === 'episode' ? spot.value.grandparentTitle : spot.value.title) : ''));
function playSpot() {
  const it = spot.value;
  if (it) router.push({ name: 'play', params: { id: it.id }, query: { t: String(Math.floor(resumeAt(it))) } });
}
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
      <section v-if="spot" class="spot">
        <img :src="spotArt" alt="" class="spot-art" @error="($event.target as HTMLElement).remove()">
        <div class="spot-text">
          <h1>{{ spotTitle }}</h1>
          <p v-if="spot.type === 'episode'" class="dim">{{ t('S{n}', { n: spot.parentIndex || 0 }) }} {{ t('E{n}', { n: spot.index || 0 }) }} · {{ spot.title }}</p>
          <p v-if="spot.summary" class="plot clamp">{{ spot.summary }}</p>
          <div class="actions">
            <button class="primary big" @click="playSpot">{{ resumeAt(spot) ? t('▶ Resume') : t('▶ Play') }}</button>
            <RouterLink class="btn big" :to="'/item/' + spot.id">{{ t('More info') }}</RouterLink>
          </div>
        </div>
      </section>
      <template v-for="(h, i) in hubs" :key="h.hubIdentifier">
        <ComingRow v-if="i === comingAt" :entries="coming" />
        <PosterRow :title="t(h.title)" :items="h.Metadata || []" :wide="wideRow(h)" />
      </template>
      <ComingRow v-if="comingAt >= hubs.length" :entries="coming" />
      <p v-if="!hubs.length && session.sections.length" class="empty">{{ t('Nothing here yet. Libraries are still being scanned, or they are empty.') }}</p>
    </template>
  </div>
</template>
