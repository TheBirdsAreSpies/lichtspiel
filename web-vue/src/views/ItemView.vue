<script setup lang="ts">
// A movie, show, season or episode: backdrop, poster, details, play / resume, watched, favorite and
// watchlist; a show's seasons with their episodes, a season's episodes, the cast and related rows.
import { computed, ref, watch } from 'vue';
import { useRouter } from 'vue-router';
import { api, get, list, picture, type Hub, type Item, type Person } from '../api';
import { fmtDur, fmtTime, isPlayable, isWatched, resumeAt, seasonTitle } from '../format';
import { lang, t } from '../i18n';
import AddToDialog from '../components/AddToDialog.vue';
import Icon from '../components/Icon.vue';
import PosterRow from '../components/PosterRow.vue';

const props = defineProps<{ id: string }>();
const router = useRouter();
const it = ref<Item | null>(null);
const kids = ref<Item[]>([]);
const season = ref<Item | null>(null);
const episodes = ref<Item[]>([]);
const top = ref<Item | null>(null);
const related = ref<Hub[]>([]);
const error = ref('');
const adding = ref(false);

async function load() {
  error.value = '';
  try {
    const item = list<Item>(await get('/library/metadata/' + props.id))[0];
    if (!item) { error.value = t('Not found'); return; }
    const topId = item.grandparentRatingKey || (item.type === 'season' ? item.parentRatingKey : null);
    const [k, tp] = await Promise.all([
      ['show', 'season'].includes(item.type) ? get(`/library/metadata/${item.id}/children`).then(j => list<Item>(j)) : Promise.resolve([] as Item[]),
      topId ? get('/library/metadata/' + topId).then(j => list<Item>(j)[0] || null).catch(() => null) : Promise.resolve(null),
    ]);
    it.value = item; kids.value = k; top.value = tp;
    // A show opens on the first season with something left to watch.
    if (item.type === 'show' && k.length && !season.value) season.value = k.find(s => !isWatched(s) && (s.index || 0) > 0) || k.find(s => (s.index || 0) > 0) || k[0];
    if (item.type === 'movie' || item.type === 'show') {
      get(`/library/metadata/${item.id}/related?count=20`).then(j => { related.value = list<Hub>(j, 'Hub'); }).catch(() => {});
    }
  } catch (e) { error.value = (e as Error).message; }
}
watch(() => props.id, () => { season.value = null; related.value = []; load(); }, { immediate: true });
watch(season, async s => {
  episodes.value = [];
  if (s) episodes.value = list<Item>(await get(`/library/metadata/${s.id}/children`).catch(() => null));
});

const x = computed(() => (it.value && it.value.extra) || {});
const cast = computed<Person[]>(() => x.value.cast || (it.value && it.value.type !== 'movie' && top.value && top.value.extra ? top.value.extra.cast || [] : []));
const art = computed(() => (it.value ? picture(`/library/metadata/${it.value.type === 'episode' ? it.value.grandparentRatingKey : it.value.id}/art?v=${it.value.updatedAt || 0}`) : ''));
const heading = computed(() => {
  const i = it.value; if (!i) return '';
  return i.type === 'episode' ? `${i.index}. ${i.title}` : i.type === 'season' ? seasonTitle(i.title) : i.title;
});
const facts = computed(() => {
  const i = it.value; if (!i) return '';
  return [i.year, i.editionTitle, i.type === 'episode' ? t('Season {n}', { n: i.parentIndex || 0 }) : null, fmtDur(i.duration), i.contentRating,
    i.rating ? `★ ${i.rating.toLocaleString(lang.value)}` : null,
    i.leafCount ? t('{seen}/{all} watched', { seen: i.viewedLeafCount || 0, all: i.leafCount }) : null].filter(Boolean).join(' · ');
});
const crew = computed(() => ([[t('Directed by'), x.value.directors], [t('Written by'), x.value.writers], [t('Created by'), x.value.creators]] as [string, string[] | undefined][])
  .filter(r => r[1] && r[1].length) as [string, string[]][]);

function play(item: Item, at: number) {
  router.push({ name: 'play', params: { id: item.id }, query: { t: String(Math.floor(at)) } });
}
// After a long break: the last minutes of the episode before (up to its credits), then this one
// from where it stopped. The player plays the one before without saving anything about it.
function catchUp(item: Item) {
  const c = item.catchUp; if (!c) return;
  router.push({ name: 'play', params: { id: c.id }, query: { t: String(Math.floor(c.from / 1000)), until: String(Math.floor(c.to / 1000)), then: String(item.id), at: String(Math.floor(resumeAt(item))) } });
}
// A show or season: the next episode not watched yet (or the first).
async function playNext() {
  const i = it.value; if (!i) return;
  const all = list<Item>(await get(`/library/metadata/${i.id}/allLeaves`).catch(() => null));
  const eps = all.length ? all : episodes.value;
  const next = eps.find(e => (e.viewOffset || 0) > 0 && !e.viewCount) || eps.find(e => !e.viewCount) || eps[0];
  if (next) play(next, resumeAt(next));
}
async function toggleWatched() {
  const i = it.value; if (!i) return;
  await api('POST', `/:/${isWatched(i) ? 'unscrobble' : 'scrobble'}?key=${i.id}`);
  await load();
  if (season.value) season.value = kids.value.find(k => k.id === season.value!.id) || season.value;
}
async function toggleFavorite() {
  const i = it.value; if (!i) return;
  await api('PUT', `/api/items/${i.id}/favorite`, { favorite: !i.favorite });
  i.favorite = !i.favorite;
}
async function toggleWatchlist() {
  const i = it.value; if (!i) return;
  await api('PUT', `/api/items/${i.id}/watchlist`, { watchlist: !i.watchlist });
  i.watchlist = !i.watchlist;
}
// The collections it is in, each a link to its library filtered to it.
const collections = computed(() => (x.value.collections || []).filter(Boolean));
const initials = (n?: string) => (n || '?').split(' ').map(w => w[0]).slice(0, 2).join('');
</script>

<template>
  <div>
    <p v-if="error" class="err">{{ error }}</p>
    <div v-else-if="!it" class="detail"><div class="poster skel"></div><div class="info"><div class="skel line"></div><div class="skel line short"></div></div></div>
    <template v-else>
      <img :src="art" alt="" class="hero" @load="($event.target as HTMLElement).classList.add('in')" @error="($event.target as HTMLElement).remove()">
      <div class="crumbs dim">
        <template v-if="it.grandparentTitle"><RouterLink :to="'/item/' + it.grandparentRatingKey">{{ it.grandparentTitle }}</RouterLink> › </template>
        <RouterLink v-if="it.parentTitle && it.parentRatingKey" :to="'/item/' + it.parentRatingKey">{{ seasonTitle(it.parentTitle) }}</RouterLink>
      </div>
      <div class="detail">
        <div class="poster" :class="{ playable: isPlayable(it) }" @click="isPlayable(it) && play(it, resumeAt(it))">
          <img v-if="it.thumb" :src="picture(it.thumb, 600)" alt="">
          <button v-if="isPlayable(it)" class="play" :aria-label="t('Play')"><Icon name="play" /></button>
        </div>
        <div class="info">
          <h1>{{ heading }}</h1>
          <div class="dim">{{ facts }}</div>
          <p v-if="it.tagline" class="tagline">{{ it.tagline }}</p>
          <p v-if="it.summary" class="plot">{{ it.summary }}</p>
          <div v-if="it.genres && it.genres.length"><span v-for="g in it.genres" :key="g" class="tag">{{ g }}</span></div>
          <div v-if="crew.length || it.studio || collections.length" class="kv">
            <template v-for="[k, v] in crew" :key="k"><div class="dim">{{ k }}</div><div>{{ v.join(', ') }}</div></template>
            <template v-if="it.studio"><div class="dim">{{ it.type === 'show' ? t('Network') : t('Studio') }}</div><div>{{ it.studio }}</div></template>
            <template v-if="collections.length && it.librarySectionID">
              <div class="dim">{{ t('Collections') }}</div>
              <div><template v-for="(c, i) in collections" :key="c"><template v-if="i">, </template><RouterLink :to="{ path: '/library/' + it.librarySectionID, query: { collection: c } }" class="link">{{ c }}</RouterLink></template></div>
            </template>
          </div>
          <div class="actions">
            <button v-if="isPlayable(it) && it.missing" class="big" disabled>{{ t('Unavailable') }}</button>
            <template v-else-if="isPlayable(it)">
              <button v-if="resumeAt(it)" class="primary big" @click="play(it, resumeAt(it))">{{ t('▶ Resume from {time}', { time: fmtTime(resumeAt(it)) }) }}</button>
              <button v-if="resumeAt(it)" class="iconbtn round" :title="t('▶ Play from start')" @click="play(it, 0)"><Icon name="restart" /></button>
              <button v-else class="primary big" @click="play(it, 0)">{{ t('▶ Play') }}</button>
              <button v-if="it.catchUp" class="big" :title="t('The last minutes of S{s}E{e} before its credits, then this episode', { s: it.catchUp.parentIndex || 0, e: it.catchUp.index || 0 })"
                @click="catchUp(it)">{{ t('What happened so far') }}</button>
            </template>
            <button v-else-if="it.leafCount" class="primary big" @click="playNext">{{ t('▶ Play next episode') }}</button>
            <button class="iconbtn round" :class="{ on: isWatched(it) }" :title="isWatched(it) ? t('Mark as unwatched') : t('Mark as watched')" @click="toggleWatched"><Icon name="check" /></button>
            <button class="iconbtn round" :class="{ on: it.favorite }" :title="it.favorite ? t('Remove from favorites') : t('Add to favorites')" @click="toggleFavorite"><Icon :name="it.favorite ? 'heart' : 'heartOff'" /></button>
            <button class="iconbtn round" :class="{ on: it.watchlist }" :title="it.watchlist ? t('Remove from watchlist') : t('Add to watchlist')" @click="toggleWatchlist"><Icon :name="it.watchlist ? 'mark' : 'markOff'" /></button>
            <button v-if="['movie', 'show', 'season', 'episode'].includes(it.type)" class="iconbtn round" :title="t('Add to …')" :aria-label="t('Add to …')" @click="adding = true"><Icon name="playlistAdd" /></button>
          </div>
        </div>
      </div>

      <template v-if="it.type === 'show' && kids.length">
        <div class="tabs">
          <button v-for="s in kids" :key="s.id" :class="{ on: season && season.id === s.id }" @click="season = s">{{ seasonTitle(s.title) }}</button>
        </div>
      </template>
      <div v-if="(it.type === 'show' && episodes.length) || (it.type === 'season' && kids.length)" class="episodes">
        <div v-for="e in (it.type === 'show' ? episodes : kids)" :key="e.id" class="ep" :class="{ missing: e.missing }">
          <RouterLink :to="'/item/' + e.id" class="ep-thumb">
            <img v-if="e.thumb" :src="picture(e.thumb, 400)" loading="lazy" alt="">
            <div v-if="e.viewOffset && e.duration" class="bar"><i :style="{ width: Math.min(100, e.viewOffset / e.duration * 100) + '%' }"></i></div>
          </RouterLink>
          <div class="grow">
            <RouterLink :to="'/item/' + e.id"><b>{{ e.index ? `${e.index}. ${e.title}` : e.title }}</b></RouterLink>
            <div class="dim">{{ [e.missing ? t('Unavailable') : '', fmtDur(e.duration), e.viewCount ? t('✓ watched') : '', resumeAt(e) ? t('stopped at {time}', { time: fmtTime(resumeAt(e)) }) : ''].filter(Boolean).join(' · ') }}</div>
            <p v-if="e.summary" class="ep-plot">{{ e.summary }}</p>
          </div>
          <button v-if="!e.missing" class="iconbtn round" :aria-label="t('Play')" @click="play(e, resumeAt(e))"><Icon name="play" /></button>
        </div>
      </div>

      <section v-if="cast.length" class="row">
        <div class="row-head"><h2>{{ t('Cast') }}</h2></div>
        <div class="strip">
          <RouterLink v-for="(p, i) in cast" :key="i" class="person" :to="{ path: '/person/' + encodeURIComponent(p.key || 'byname'), query: { name: p.name || '' } }">
            <img v-if="p.thumb || p.photo" :src="p.thumb ? picture(p.thumb) : p.photo" alt="" loading="lazy" referrerpolicy="no-referrer">
            <div v-else class="initials">{{ initials(p.name) }}</div>
            <b>{{ p.name }}</b>
            <div v-if="p.role" class="dim">{{ p.role }}</div>
          </RouterLink>
        </div>
      </section>
      <PosterRow v-for="h in related" :key="h.hubIdentifier + h.title" :title="t(h.title)" :items="h.Metadata || []" />
      <AddToDialog v-if="adding" :item="it" @close="adding = false" @changed="load" />
    </template>
  </div>
</template>
