<script setup lang="ts">
// A movie, show, season or episode: backdrop, poster, details, play / resume, watched, favorite and
// watchlist; a show's seasons with their episodes, a season's episodes, the cast and related rows.
import { computed, onBeforeUnmount, ref, watch } from 'vue';
import { useRouter } from 'vue-router';
import { api, get, list, picture, session, type Hub, type Item, type Person } from '../api';
import { fmtDur, fmtSize, fmtTime, isPlayable, isWatched, resumeAt, seasonTitle } from '../format';
import { lang, t } from '../i18n';
import AddToDialog from '../components/AddToDialog.vue';
import EditDetails from '../components/EditDetails.vue';
import DownloadDialog from '../components/DownloadDialog.vue';
import TogetherDialog from '../components/TogetherDialog.vue';
import ReportDialog from '../components/ReportDialog.vue';
import ItemMenu from '../components/ItemMenu.vue';
import ScoreBadges from '../components/ScoreBadges.vue';
import StarRating from '../components/StarRating.vue';
import Modal from '../components/Modal.vue';
import { startTheme, stopTheme, theme, toggleThemeMute } from '../theme';
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
const editing = ref(false);
const downloading = ref(false);
const together = ref(false);
const reporting = ref(false);

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
    startTheme(String(item.id));
    // A show opens on the first season with something left to watch.
    if (item.type === 'show' && k.length && !season.value) season.value = k.find(s => !isWatched(s) && (s.index || 0) > 0) || k.find(s => (s.index || 0) > 0) || k[0];
    if (item.type === 'movie' || item.type === 'show') {
      get(`/library/metadata/${item.id}/related?count=20`).then(j => { related.value = list<Hub>(j, 'Hub'); }).catch(() => {});
    }
  } catch (e) { error.value = (e as Error).message; }
}
watch(() => props.id, () => { season.value = null; related.value = []; load(); }, { immediate: true });
onBeforeUnmount(stopTheme);
// Another season: its episodes take the place of the shown ones once they are there (the old ones stay,
// dimmed, meanwhile, so nothing jumps); seasons seen once on this page come at once.
const seasonEps = new Map<number, Item[]>();
const epsLoading = ref(false);
const epsShown = ref(0); // which season the list shows (its key: a new list fades in)
watch(() => props.id, () => seasonEps.clear());
watch(season, async s => {
  if (!s) { episodes.value = []; return; }
  let eps = seasonEps.get(s.id);
  if (!eps) {
    epsLoading.value = true;
    eps = list<Item>(await get(`/library/metadata/${s.id}/children`).catch(() => null));
    if (season.value !== s) return; // another season was chosen meanwhile
    seasonEps.set(s.id, eps);
  }
  epsLoading.value = false;
  episodes.value = eps;
  epsShown.value = s.id;
});
// Watched or not changed: the seasons are asked again.
watch(kids, () => seasonEps.clear());

const x = computed(() => (it.value && it.value.extra) || {});
// (What the page's item has besides the Item type: scores, links, trailers, the state of the match.)
type Any = Record<string, any>; // eslint-disable-line @typescript-eslint/no-explicit-any
const xa = computed<Any>(() => x.value as Any);
const topX = computed<Any>(() => ((top.value && top.value.extra) || {}) as Any);
const ratings = computed(() => xa.value.ratings || (it.value && it.value.type === 'season' ? topX.value.ratings : null));
const matchable = computed(() => !!it.value && (it.value.type === 'movie' || it.value.type === 'show'));
// Without a plot, the owner learns why: no match yet, none found, or removed by hand.
const unmatchedNote = computed(() => {
  if (!it.value || it.value.summary || !session.me?.admin || !matchable.value || xa.value.metadata_source) return '';
  return xa.value.unmatched_by_hand ? t('The match was removed — use "Match" to pick one, or "Refresh metadata" to look again.')
    : xa.value.metadata_status === 'unmatched' ? t('No details were found online for this title — use "Match" to pick the right one.') : t('Details are being looked up online…');
});
// The title on other sites.
const links = computed(() => {
  const i = it.value; if (!i) return [] as [string, string][];
  const ids: Any = xa.value.ids || topX.value.ids || {};
  return ([ids.imdb ? ['IMDb', 'https://www.imdb.com/title/' + ids.imdb] : null, ids.tmdb ? ['TMDB', 'https://www.themoviedb.org/' + (i.type === 'movie' ? 'movie' : 'tv') + '/' + ids.tmdb] : null,
    i.type === 'movie' && (ids.tmdb || ids.imdb) ? ['Letterboxd', ids.tmdb ? 'https://letterboxd.com/tmdb/' + ids.tmdb : 'https://letterboxd.com/imdb/' + ids.imdb] : null,
    ids.tvmaze ? ['TVmaze', 'https://www.tvmaze.com/shows/' + ids.tvmaze] : null, ids.anilist ? ['AniList', 'https://anilist.co/anime/' + ids.anilist] : null,
    ids.mal ? ['MyAnimeList', 'https://myanimelist.net/anime/' + ids.mal] : null, ids.wikipedia ? ['Wikipedia', ids.wikipedia] : null,
    ids.wikidata ? ['Wikidata', 'https://www.wikidata.org/wiki/' + ids.wikidata] : null].filter(Boolean)) as [string, string][];
});
// A trailer file next to the movie first, else the first online trailer (played from YouTube in a box).
interface Online { key: string, name?: string, site?: string }
const localTrailer = computed(() => ((it.value as Any | null)?.Extras || []).find((e: Any) => e.extraType === 'trailer') as Item | undefined);
const onlineTrailer = computed<Online | null>(() => (matchable.value ? ((xa.value.trailers || []) as Online[]).find(v => v && v.key) || null : null));
const watchingOnline = ref<Online | null>(null);
function playTrailer() {
  if (localTrailer.value) play(localTrailer.value, 0); else if (onlineTrailer.value) { stopTheme(); watchingOnline.value = onlineTrailer.value; }
}
const released = computed(() => (xa.value.originally_available ? new Date(xa.value.originally_available).toLocaleDateString(lang.value) : ''));
const cast = computed<Person[]>(() => x.value.cast || (it.value && it.value.type !== 'movie' && top.value && top.value.extra ? top.value.extra.cast || [] : []));
const art = computed(() => (it.value ? picture(`/library/metadata/${it.value.type === 'episode' ? it.value.grandparentRatingKey : it.value.id}/art?v=${it.value.updatedAt || 0}`) : ''));
const heading = computed(() => {
  const i = it.value; if (!i) return '';
  return i.type === 'episode' ? `${i.index}. ${i.title}` : i.type === 'season' ? seasonTitle(i.title) : i.title;
});
const facts = computed(() => {
  const i = it.value; if (!i) return '';
  return [i.year, i.editionTitle, i.type === 'episode' ? t('Season {n}', { n: i.parentIndex || 0 }) : null, fmtDur(i.duration), i.contentRating,
    i.rating && !Object.keys(ratings.value || {}).length ? `★ ${i.rating.toLocaleString(lang.value)}` : null,
    i.leafCount ? t('{seen}/{all} watched', { seen: i.viewedLeafCount || 0, all: i.leafCount }) : null].filter(Boolean).join(' · ');
});
const crew = computed(() => ([[t('Directed by'), x.value.directors], [t('Written by'), x.value.writers], [t('Created by'), x.value.creators]] as [string, string[] | undefined][])
  .filter(r => r[1] && r[1].length) as [string, string[]][]);

// Merged by hand (the owner's "Merge"): several files of the same title. Which one plays is chosen
// here and kept per title in this browser (as in the classic app, under the same key).
const versions = computed(() => (it.value && it.value.Media && it.value.Media.length > 1 ? it.value.Media : null));
const version = ref(0);
watch(() => props.id, id => {
  let v = 0; try { v = Number(localStorage.getItem('version:' + id)) || 0; } catch { /* none */ }
  version.value = v;
}, { immediate: true });
function setVersion(v: number) { version.value = v; try { localStorage.setItem('version:' + props.id, String(v)); } catch { /* none */ } }
const versionName = (m: NonNullable<Item['Media']>[number]) => [m.height ? (m.height >= 2000 ? '4K' : m.height + 'p') : '', m.edition, m.size ? fmtSize(m.size) : ''].filter(Boolean).join(' · ');
// The id that plays for this title: the chosen version's (each version is an item of its own).
const playId = (item: Item) => {
  const v = versions.value;
  return item === it.value && v && version.value > 0 && version.value < v.length && v[version.value].id ? v[version.value].id! : item.id;
};
function play(item: Item, at: number) {
  router.push({ name: 'play', params: { id: playId(item) }, query: { t: String(Math.floor(at)) } });
}
// After a long break: the last minutes of the episode before (up to its credits), then this one
// from where it stopped. The player plays the one before without saving anything about it.
function catchUp(item: Item) {
  const c = item.catchUp; if (!c) return;
  router.push({ name: 'play', params: { id: c.id }, query: { t: String(Math.floor(c.from / 1000)), until: String(Math.floor(c.to / 1000)), then: String(playId(item)), at: String(Math.floor(resumeAt(item))) } });
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
          <ScoreBadges :ratings="ratings" />
          <p v-if="it.tagline" class="tagline">{{ it.tagline }}</p>
          <p v-if="it.summary" class="plot">{{ it.summary }}</p>
          <p v-else-if="unmatchedNote" class="dim">{{ unmatchedNote }}</p>
          <div v-if="it.genres && it.genres.length"><span v-for="g in it.genres" :key="g" class="tag">{{ g }}</span></div>
          <div v-if="crew.length || it.studio || collections.length || released || (xa.original_title && xa.original_title !== it.title)" class="kv">
            <template v-for="[k, v] in crew" :key="k"><div class="dim">{{ k }}</div><div>{{ v.join(', ') }}</div></template>
            <template v-if="it.studio"><div class="dim">{{ it.type === 'show' ? t('Network') : t('Studio') }}</div><div>{{ it.studio }}</div></template>
            <template v-if="xa.original_title && xa.original_title !== it.title"><div class="dim">{{ t('Original title') }}</div><div>{{ xa.original_title }}</div></template>
            <template v-if="released"><div class="dim">{{ it.type === 'movie' ? t('Released') : t('First aired') }}</div><div>{{ released }}</div></template>
            <template v-if="collections.length && it.librarySectionID">
              <div class="dim">{{ t('Collections') }}</div>
              <div><template v-for="(c, i) in collections" :key="c"><template v-if="i">, </template><RouterLink :to="{ path: '/library/' + it.librarySectionID, query: { collection: c } }" class="link">{{ c }}</RouterLink></template></div>
            </template>
          </div>
          <div v-if="links.length" class="dim links">{{ t('More: ') }}<template v-for="([n, u], i) in links" :key="n"><template v-if="i"> · </template><a :href="u" target="_blank" rel="noopener">{{ n }}</a></template></div>
          <div class="actions">
            <button v-if="isPlayable(it) && it.missing" class="big" disabled>{{ t('Unavailable') }}</button>
            <template v-else-if="isPlayable(it)">
              <button v-if="resumeAt(it)" class="primary big" @click="play(it, resumeAt(it))">{{ t('▶ Resume from {time}', { time: fmtTime(resumeAt(it)) }) }}</button>
              <button v-if="resumeAt(it)" class="iconbtn round" :title="t('▶ Play from start')" @click="play(it, 0)"><Icon name="restart" /></button>
              <button v-else class="primary big" @click="play(it, 0)">{{ t('▶ Play') }}</button>
              <button v-if="it.catchUp" class="big" :title="t('The last minutes of S{s}E{e} before its credits, then this episode', { s: it.catchUp.parentIndex || 0, e: it.catchUp.index || 0 })"
                @click="catchUp(it)">{{ t('What happened so far') }}</button>
              <select v-if="versions" :value="Math.min(version, versions.length - 1)" :title="t('Version')" :aria-label="t('Version')" @change="setVersion(Number(($event.target as HTMLSelectElement).value))">
                <option v-for="(m, i) in versions" :key="i" :value="i">{{ versionName(m) }}</option>
              </select>
            </template>
            <button v-else-if="it.leafCount" class="primary big" @click="playNext">{{ t('▶ Play next episode') }}</button>
            <button v-if="localTrailer || onlineTrailer" class="big" @click="playTrailer">{{ t('▶ Trailer') }}</button>
            <button class="iconbtn round" :class="{ on: isWatched(it) }" :title="isWatched(it) ? t('Mark as unwatched') : t('Mark as watched')" @click="toggleWatched"><Icon name="check" /></button>
            <button class="iconbtn round" :class="{ on: it.favorite }" :title="it.favorite ? t('Remove from favorites') : t('Add to favorites')" @click="toggleFavorite"><Icon :name="it.favorite ? 'heart' : 'heartOff'" /></button>
            <button class="iconbtn round" :class="{ on: it.watchlist }" :title="it.watchlist ? t('Remove from watchlist') : t('Add to watchlist')" @click="toggleWatchlist"><Icon :name="it.watchlist ? 'mark' : 'markOff'" /></button>
            <button v-if="['movie', 'show', 'season', 'episode'].includes(it.type)" class="iconbtn round" :title="t('Add to …')" :aria-label="t('Add to …')" @click="adding = true"><Icon name="playlistAdd" /></button>
            <button v-if="['movie', 'show', 'season', 'episode'].includes(it.type)" class="iconbtn round" :title="t('Watch together')" :aria-label="t('Watch together')" @click="together = true"><Icon name="people" /></button>
            <button v-if="session.me?.can_download && ['movie', 'show', 'season', 'episode'].includes(it.type)" class="iconbtn round" :title="t('Download…')" :aria-label="t('Download…')" @click="downloading = true"><Icon name="download" /></button>
            <button v-if="['movie', 'show', 'season', 'episode'].includes(it.type)" class="iconbtn round" :title="t('Report a problem')" :aria-label="t('Report a problem')" @click="reporting = true"><Icon name="flag" /></button>
            <button v-if="session.me?.admin && it.type !== 'clip'" class="iconbtn round" :title="t('Edit details')" :aria-label="t('Edit details')" @click="editing = true"><Icon name="edit" /></button>
            <ItemMenu v-if="['movie', 'show', 'season', 'episode'].includes(it.type)" :item="it" @changed="load" />
            <button v-if="theme.playing" class="iconbtn round" :title="theme.muted ? t('Theme music on') : t('Mute theme music')" :aria-label="theme.muted ? t('Theme music on') : t('Mute theme music')" @click="toggleThemeMute"><Icon :name="theme.muted ? 'mute' : 'vol'" /></button>
          </div>
          <StarRating v-if="['movie', 'show', 'season', 'episode'].includes(it.type)" :id="it.id" :key="it.id" :rating="(it as Any).userRating" />
        </div>
      </div>

      <template v-if="it.type === 'show' && kids.length">
        <!-- Several seasons: tabs as pills, the one shown filled; only one: the heading says whose episodes these are. -->
        <h2 v-if="kids.length > 1" class="gap-top">{{ t('Seasons') }}</h2>
        <h2 v-else class="gap-top">{{ t('Episodes') }} <span class="dim normal">· {{ seasonTitle(kids[0].title) }}</span></h2>
        <div v-if="kids.length > 1" class="stabs" role="tablist">
          <button v-for="s in kids" :key="s.id" type="button" role="tab" :aria-selected="!!season && season.id === s.id" :class="{ on: season && season.id === s.id }" @click="season = s">
            {{ seasonTitle(s.title) }}<small v-if="s.leafCount && (s.viewedLeafCount || 0) > 0 && (s.viewedLeafCount || 0) < s.leafCount" :title="t('{n} not seen', { n: s.leafCount - (s.viewedLeafCount || 0) })">{{ s.leafCount - (s.viewedLeafCount || 0) }}</small>
          </button>
        </div>
      </template>
      <h2 v-else-if="it.type === 'season' && kids.length" class="gap-top">{{ t('Episodes') }}</h2>
      <!-- The episodes as cards: the still (a click plays), its number and name, the length, the plot. -->
      <div v-if="(it.type === 'show' && episodes.length) || (it.type === 'season' && kids.length)" :key="it.type === 'show' ? epsShown : it.id" class="eps stagger" :class="{ loading: epsLoading }">
        <div v-for="(e, i) in (it.type === 'show' ? episodes : kids)" :key="e.id" class="card ep-card" :class="{ watched: !!e.viewCount, missing: e.missing }" :style="{ '--i': i }">
          <div class="poster" @click="!e.missing && play(e, resumeAt(e))">
            <img v-if="e.thumb" :src="picture(e.thumb, 480)" loading="lazy" alt="" @load="($event.target as HTMLElement).classList.add('in')">
            <div v-if="e.viewOffset && e.duration" class="bar"><i :style="{ width: Math.min(100, e.viewOffset / e.duration * 100) + '%' }"></i></div>
            <span v-if="e.viewCount" class="seen" :title="t('Watched')"><Icon name="check" /></span>
            <button v-if="!e.missing" class="play" :title="t('Play')" :aria-label="t('Play')" @click.stop="play(e, resumeAt(e))"><Icon name="play" /></button>
          </div>
          <RouterLink :to="'/item/' + e.id" class="t" :title="e.title">{{ e.index ? `${e.index}. ${e.title}` : e.title }}</RouterLink>
          <div class="s">{{ [e.missing ? t('Unavailable') : '', fmtDur(e.duration), resumeAt(e) ? t('stopped at {time}', { time: fmtTime(resumeAt(e)) }) : ''].filter(Boolean).join(' · ') }}</div>
          <p v-if="e.summary" class="ep-plot">{{ e.summary }}</p>
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
      <DownloadDialog v-if="downloading" :item="it" @close="downloading = false" />
      <TogetherDialog v-if="together" :item="it" @close="together = false" />
      <ReportDialog v-if="reporting" :item="it" @close="reporting = false" />
      <EditDetails v-if="editing" :id="it.id" @close="editing = false" @saved="load" />
      <Modal v-if="watchingOnline" :title="watchingOnline.name || t('Trailer')" wide @close="watchingOnline = null">
        <div class="yt"><iframe :src="`https://www.youtube-nocookie.com/embed/${encodeURIComponent(watchingOnline.key)}?autoplay=1&rel=0`" :title="watchingOnline.name || 'Trailer'"
          allow="autoplay; encrypted-media; picture-in-picture; fullscreen" allowfullscreen referrerpolicy="strict-origin-when-cross-origin"></iframe></div>
        <p class="dim small">{{ t('Online video from {site}', { site: watchingOnline.site || 'YouTube' }) }}</p>
      </Modal>
    </template>
  </div>
</template>
