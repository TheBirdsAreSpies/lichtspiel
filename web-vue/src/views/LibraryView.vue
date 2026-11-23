<script setup lang="ts">
// A library: every title as a grid of posters, loaded page by page while scrolling, with sorting, the
// filters of the library (in the address: ?f.genre=Drama) and a search inside the library. Sorting and
// filters are kept per library and come back on the next visit.
// The tab "Collections" shows its collections (film series and the ones made by hand); one opens as the
// library filtered to it (?collection=…).
import { computed, nextTick, onBeforeUnmount, onMounted, ref, watch } from 'vue';
import { useRoute, useRouter } from 'vue-router';
import { get, list, picture, session, type Group, type Item } from '../api';
import { CARD, cardSize, setCardSize } from '../cardSize';
import { chipLabel, fromQuery, type Rule } from '../filters';
import { t } from '../i18n';
import AdvancedFilters from '../components/AdvancedFilters.vue';
import FilterButton from '../components/FilterButton.vue';
import PosterCard from '../components/PosterCard.vue';
import RandomPick from '../components/RandomPick.vue';
import PickBar from '../components/PickBar.vue';

const props = defineProps<{ id: string }>();
const PAGE = 60;
// What to sort by (as the classic app), each with the direction it starts in; the arrow beside turns it.
const SORT_FIELDS: [string, string][] = [['title', 'Title'], ['year', 'Year'], ['release_date', 'Release date'], ['critic_rating', 'Critic rating'], ['audience_rating', 'Audience rating'],
  ['user_rating', 'Your rating'], ['content_rating', 'Age rating'], ['duration', 'Length'], ['progress', 'Progress'], ['view_count', 'Plays'], ['added_at', 'Date added'],
  ['last_viewed_at', 'Date played'], ['resolution', 'Resolution'], ['bitrate', 'Bitrate'], ['random', 'Random']];
const SORT_ASC = new Set(['title', 'content_rating', 'random']);
// (Older kept choices: rating became the critics' one.)
const OLD_SORT: Record<string, string> = { rating: 'critic_rating' };

const route = useRoute();
const router = useRouter();
const tab = computed(() => (route.query.tab === 'collections' ? 'collections' : ''));
const collection = computed(() => (typeof route.query.collection === 'string' ? route.query.collection : ''));
const section = computed(() => session.sections.find(s => String(s.id) === props.id));
const kept = (() => { try { return JSON.parse(localStorage.getItem('vue-lib:' + props.id) || '{}'); } catch { return {}; } })();
const sort = ref<string>((() => { const [f, d] = String(kept.sort || 'title:asc').split(':'); const g = OLD_SORT[f] || f; return SORT_FIELDS.some(x => x[0] === g) ? g + ':' + (d || 'asc') : 'title:asc'; })());
const sortField = computed({ get: () => sort.value.split(':')[0], set: f => { sort.value = f + ':' + (SORT_ASC.has(f) ? 'asc' : 'desc'); } });
const sortDesc = computed(() => sort.value.endsWith(':desc'));
function turnSort() { sort.value = sortField.value + ':' + (sortDesc.value ? 'asc' : 'desc'); }

// The dice: one title at random from what the filters leave.
const rolled = ref<Item | null>(null);
async function randomPick() {
  if (!total.value) return;
  const p = filterParams(); p.set('sort', sort.value); p.set('start', String(Math.floor(Math.random() * total.value))); p.set('size', '1');
  try { rolled.value = list<Item>(await get(`/library/sections/${props.id}/all?${p}`))[0] || null; } catch (e) { error.value = (e as Error).message; }
}
const active = computed(() => fromQuery(route.query));
// The advanced filters (rules) in the address: ?rules=[{field, op, value}…]&rmatch=any.
const rulesText = computed(() => (typeof route.query.rules === 'string' ? route.query.rules : ''));
const rmatch = computed(() => route.query.rmatch === 'any');
const rules = computed<Rule[]>(() => { try { return rulesText.value ? JSON.parse(rulesText.value) : []; } catch { return []; } });
const advancedOpen = ref(false);
const q = ref('');
// Every title has its place from the first page on (null until its page is loaded), so the scroll bar is
// right at once and a letter can jump anywhere; the empty cards near the screen load their page.
const items = ref<(Item | null)[]>([]);
const total = ref<number | null>(null);
const loading = ref(false);
const error = ref('');
const gridEl = ref<HTMLElement | null>(null);
let seq = 0;
const have = new Set<number>(), busy = new Set<number>();

/** The filters as a query (without sorting and paging; the letters use it as well). */
function filterParams() {
  const p = new URLSearchParams();
  for (const [f, v] of Object.entries(active.value)) p.set(f, v);
  if (q.value.trim()) p.set('q', q.value.trim());
  if (collection.value) p.set('collection', collection.value);
  if (rules.value.length) { p.set('rules', rulesText.value); if (rmatch.value) p.set('rmatch', 'any'); }
  return p;
}
function url(start: number) {
  const p = filterParams();
  p.set('sort', sort.value); p.set('start', String(start)); p.set('size', String(PAGE));
  return `/library/sections/${props.id}/all?${p}`;
}

async function loadPage(n: number) {
  if (have.has(n) || busy.has(n)) return;
  busy.add(n);
  const mine = seq;
  if (n === 0) loading.value = true;
  try {
    const j = await get<{ MediaContainer: { totalSize?: number } }>(url(n * PAGE));
    if (mine !== seq) return;
    const got = list<Item>(j);
    if (total.value === null) {
      total.value = j.MediaContainer.totalSize ?? got.length;
      items.value = new Array<Item | null>(total.value).fill(null);
      loadLetters();
    }
    items.value.splice(n * PAGE, got.length, ...got);
    have.add(n);
  } catch (e) { if (mine === seq) error.value = (e as Error).message; } finally {
    if (mine === seq) { busy.delete(n); if (n === 0) loading.value = false; }
  }
}

// ---- selecting titles (the circle on a poster, Ctrl or Shift with a click): the bar at the bottom acts on them.
const picked = ref(new Set<number>());
let lastPick = -1;
function pick(i: number, e: MouseEvent) {
  const s = new Set(picked.value);
  if (e.shiftKey && lastPick >= 0) { for (let k = Math.min(lastPick, i); k <= Math.max(lastPick, i); k++) if (items.value[k]) s.add(k); }
  else if (s.has(i)) s.delete(i); else s.add(i);
  lastPick = i; picked.value = s;
}
const pickedItems = computed(() => [...picked.value].sort((a, b) => a - b).map(i => items.value[i]).filter((x): x is Item => !!x));
function clearPicks() { picked.value = new Set(); lastPick = -1; }
function pickAll() { picked.value = new Set(items.value.map((x, i) => (x ? i : -1)).filter(i => i >= 0)); }
// Titles that changed (watched, edited, merged): read again where they are; none given: the whole list.
async function refreshPicked(ids: number[]) {
  if (!ids.length) { restart(); return; }
  for (const id of ids) {
    const i = items.value.findIndex(x => x && x.id === id);
    if (i < 0) continue;
    try { const fresh = list<Item>(await get('/library/metadata/' + id))[0]; if (fresh) items.value.splice(i, 1, { ...items.value[i], ...fresh }); } catch { /* keep it */ }
  }
}
const onKey = (e: KeyboardEvent) => { if (e.key === 'Escape' && picked.value.size) clearPicks(); };

function restart() {
  clearPicks();
  seq++; items.value = []; total.value = null; loading.value = false; error.value = '';
  have.clear(); busy.clear(); marks.value = [];
  // (Not while the collections show: their address has no filters, and the kept ones stay.)
  if (!tab.value) try { localStorage.setItem('vue-lib:' + props.id, JSON.stringify({ sort: sort.value, filters: active.value, rules: rulesText.value, rmatch: rmatch.value })); } catch { /* none */ }
  loadPage(0);
}
let typing = 0;
watch([sort, () => JSON.stringify(active.value), collection, rulesText, rmatch], restart);

/** Puts a filter on (or takes it off with ''); the collections tab gives way to the titles. */
function setFilter(f: string, v: string) {
  const q: Record<string, string> = {};
  for (const [k, x] of Object.entries(route.query)) if (typeof x === 'string' && k !== 'tab' && k !== 'f.' + f) q[k] = x;
  if (v) q['f.' + f] = v;
  router.replace({ query: q });
}
/** Kept filters as an address query (?f.genre=…, the rules). */
function asQuery(k: { filters?: Record<string, string>, rules?: string, rmatch?: boolean }): Record<string, string> {
  const q: Record<string, string> = Object.fromEntries(Object.entries(k.filters || {}).map(([f, v]) => ['f.' + f, v]));
  if (k.rules) { q.rules = k.rules; if (k.rmatch) q.rmatch = 'any'; }
  return q;
}
/** The kept filters as an address query, for going back from the collections to the titles. */
function keptQuery(): Record<string, string> {
  try { return asQuery(JSON.parse(localStorage.getItem('vue-lib:' + props.id) || '{}')); } catch { return {}; }
}
function clearFilters() {
  const q: Record<string, string> = {};
  for (const [k, x] of Object.entries(route.query)) if (typeof x === 'string' && !k.startsWith('f.') && k !== 'rules' && k !== 'rmatch') q[k] = x;
  router.replace({ query: q });
}
/** Applies the advanced filters (none: takes them off). */
function setRules(list: Rule[], any: boolean) {
  advancedOpen.value = false;
  const q: Record<string, string> = {};
  for (const [k, x] of Object.entries(route.query)) if (typeof x === 'string' && k !== 'tab' && k !== 'rules' && k !== 'rmatch') q[k] = x;
  if (list.length) { q.rules = JSON.stringify(list); if (any) q.rmatch = 'any'; }
  router.replace({ query: q });
}
// Opened without a choice: the filters of the last visit come back (an older "unwatched" tick too).
const saved: Record<string, string> = kept.filters || (kept.unwatched ? { [section.value && section.value.type === 'show' ? 'unwatched_leaves' : 'unwatched']: 'true' } : {});
const restoring = !tab.value && !collection.value && !Object.keys(active.value).length && !rulesText.value && (Object.keys(saved).length > 0 || !!kept.rules);
if (restoring) router.replace({ query: asQuery({ filters: saved, rules: kept.rules, rmatch: kept.rmatch }) });
watch(q, () => { clearTimeout(typing); typing = window.setTimeout(restart, 300); });

// Empty cards that come near the screen load their page, unless they are only passed (dragging the scroll bar).
const near = new Set<Element>();
const io = new IntersectionObserver(es => {
  for (const e of es) {
    if (!e.isIntersecting) { near.delete(e.target); continue; }
    near.add(e.target);
    window.setTimeout(() => {
      if (!near.has(e.target) || !e.target.isConnected) return;
      near.delete(e.target);
      loadPage(Math.floor(Number((e.target as HTMLElement).dataset.i) / PAGE));
    }, 150);
  }
}, { rootMargin: '900px' });
/** v-pending on an empty card: watched while it is there. */
const vPending = { mounted: (el: Element) => io.observe(el), beforeUnmount: (el: Element) => { io.unobserve(el); near.delete(el); } };

// ---- the letters at the side (sorted by title): a click jumps to the letter's first title; on a phone the
// finger, resting a moment on the bar, runs down it and the list follows.
interface Mark { key: string, size: number, ranges: [number, number][] }
const marks = ref<Mark[]>([]);
const current = ref('');
const letterBox = ref<HTMLElement | null>(null);
async function loadLetters() {
  if (sort.value !== 'title:asc' || (total.value || 0) <= PAGE / 2) return;
  const mine = seq;
  try {
    const r = await get<{ MediaContainer: { Directory?: { key: string, size: number, start?: number, end?: number }[] } }>(`/library/sections/${props.id}/firstCharacter?${filterParams()}`);
    if (mine !== seq) return;
    // A letter can have several runs in the list (Ä/Ö/Ü sort behind Z): one button, all its places.
    const by = new Map<string, Mark>();
    let sum = 0;
    for (const d of r.MediaContainer.Directory || []) {
      const from = d.start ?? sum, to = d.end ?? sum + d.size;
      sum += d.size;
      const m = by.get(d.key) || { key: d.key, size: 0, ranges: [] };
      m.size += d.size; m.ranges.push([from, to]); by.set(d.key, m);
    }
    marks.value = [...by.values()];
    onScroll();
  } catch { /* no letters */ }
}
const firstOf = (m: Mark) => Math.min(...m.ranges.map(r => r[0]));
// The letter just chosen stays lit for a moment, whatever is under the top bar.
let forced = '', forcedAt = 0;
async function jump(m: Mark) {
  const i = Math.min(firstOf(m), (total.value || 1) - 1);
  forced = m.key; forcedAt = Date.now(); current.value = m.key;
  const at = () => {
    const el = gridEl.value && gridEl.value.querySelector(`[data-i="${i}"]`);
    if (!el) return null;
    const top = parseFloat(getComputedStyle(document.documentElement).getPropertyValue('--top')) || 60;
    window.scrollTo({ top: el.getBoundingClientRect().top + window.scrollY - top - 12, behavior: 'auto' });
    return window.scrollY;
  };
  const y = at();
  await loadPage(Math.floor(i / PAGE));
  await nextTick();
  // The real cards may be a little taller than the empty ones: line up again, if nobody scrolled meanwhile.
  if (y !== null && Math.abs(window.scrollY - y) < 3) at();
}
// Where you are: the first card under the top bar lights its letter.
let raf = 0;
function onScroll() {
  if (raf || !marks.value.length) return;
  raf = requestAnimationFrame(() => {
    raf = 0;
    if (forced && Date.now() - forcedAt < 800) return;
    forced = '';
    const g = gridEl.value; if (!g) return;
    const r = g.getBoundingClientRect();
    const top = parseFloat(getComputedStyle(document.documentElement).getPropertyValue('--top')) || 60;
    const hit = document.elementFromPoint(r.left + 20, Math.max(top + 40, r.top + 20));
    const card = hit && (hit.closest('[data-i]') as HTMLElement | null);
    if (!card) return;
    const i = Number(card.dataset.i);
    const m = marks.value.find(x => x.ranges.some(([a, b]) => i >= a && i < b));
    if (m) current.value = m.key;
  });
}
const bubble = ref('');
const bubbleTop = ref(0);
let scrubbing = false, holdTimer = 0, holdY = 0, lastY = 0, scrubKey = '';
function scrubAt(y: number) {
  bubbleTop.value = Math.round(Math.max(70, Math.min(innerHeight - 170, y - 110)));
  const box = letterBox.value; if (!box) return;
  let best: Mark | null = null, dist = Infinity;
  box.querySelectorAll('button').forEach((b, n) => {
    const r = b.getBoundingClientRect(), d = Math.abs(y - (r.top + r.height / 2));
    if (d < dist) { dist = d; best = marks.value[n]; }
  });
  const m = best as Mark | null;
  if (!m || m.key === scrubKey) return;
  scrubKey = m.key; bubble.value = m.key;
  jump(m);
}
// The bar sits where a thumb scrolls the page, so it only takes the finger after it rests a moment; a finger
// that moves at once scrolls the page as usual. A tap still jumps to the letter.
function scrubEnd() { clearTimeout(holdTimer); holdTimer = 0; scrubbing = false; bubble.value = ''; }
function touchStart(e: TouchEvent) {
  scrubEnd();
  if (e.touches.length !== 1 || !marks.value.length) return;
  holdY = lastY = e.touches[0].clientY;
  holdTimer = window.setTimeout(() => { holdTimer = 0; scrubbing = true; scrubKey = ''; scrubAt(lastY); }, 180);
}
function touchMove(e: TouchEvent) {
  lastY = e.touches[0].clientY;
  if (scrubbing) {
    if (!e.cancelable) { scrubEnd(); return; }
    e.preventDefault();
    scrubAt(lastY);
  } else if (holdTimer && Math.abs(lastY - holdY) > 8) scrubEnd();
}
function touchEnd(e: TouchEvent) { if (scrubbing) e.preventDefault(); scrubEnd(); }

// (While the kept filters are being put into the address, the page waits for them.)
onMounted(() => {
  window.addEventListener('scroll', onScroll, { passive: true }); window.addEventListener('keydown', onKey);
  if (tab.value) loadGroups(); else if (!restoring) loadPage(0);
});
onBeforeUnmount(() => { io.disconnect(); window.removeEventListener('scroll', onScroll); window.removeEventListener('keydown', onKey); cancelAnimationFrame(raf); scrubEnd(); });

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
          <select v-model="sortField" :aria-label="t('Sort')" :title="t('Sort')">
            <option v-for="[v, l] in SORT_FIELDS" :key="v" :value="v">{{ t(l) }}</option>
          </select>
          <button v-if="sortField !== 'random'" class="dirbtn" :title="sortDesc ? t('Descending') : t('Ascending')" :aria-label="sortDesc ? t('Descending') : t('Ascending')" @click="turnSort">{{ sortDesc ? '↓' : '↑' }}</button>
          <button v-if="(total || 0) > 1" class="dirbtn" :title="t('Pick one at random')" :aria-label="t('Pick one at random')" @click="randomPick">🎲</button>
          <label class="size" :title="t('Poster size')">
            <span class="dim small-mark">▮</span>
            <input type="range" :min="CARD.min" :max="CARD.max" step="10" :value="cardSize" :aria-label="t('Poster size')" @input="setCardSize(Number(($event.target as HTMLInputElement).value))">
            <span class="dim big-mark">▮</span>
          </label>
          <FilterButton :url="`/library/sections/${id}/filters`" :active="active" :remember-key="'vue-libFilter:' + id" :advanced="rules.length" @set="setFilter" @advanced="advancedOpen = true" />
        </div>
      </div>
      <div v-if="!collection" class="tabs flat">
        <button :class="{ on: !tab }" @click="router.replace({ query: keptQuery() })">{{ t('Library') }}</button>
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
      <div v-if="Object.keys(active).length || rules.length" class="chips">
        <button v-for="(v, f) in active" :key="f" class="chip" :title="t('Remove this filter')" @click="setFilter(String(f), '')">{{ chipLabel(String(f), v) }} <span aria-hidden="true">×</span></button>
        <button v-if="rules.length" class="chip" :title="t('Advanced filters')" @click="advancedOpen = true">{{ t('Advanced filters ({n})', { n: rules.length }) }}</button>
        <button v-if="Object.keys(active).length + (rules.length ? 1 : 0) > 1" class="linkbtn" @click="clearFilters">{{ t('Clear filters') }}</button>
      </div>
      <RandomPick v-if="rolled" :item="rolled" @close="rolled = null" @again="randomPick" />
      <AdvancedFilters v-if="advancedOpen" :rules="rules" :any="rmatch" @close="advancedOpen = false" @apply="setRules" />
      <div ref="gridEl" class="grid" :class="{ 'has-letters': marks.length, picking: picked.size > 0 }">
        <template v-for="(it, i) in items" :key="it ? it.id : 'p' + i">
          <PosterCard v-if="it" :item="it" :data-i="i" pickable :picked="picked.has(i)" :picking="picked.size > 0" @pick="pick(i, $event)" />
          <div v-else v-pending class="card ph" :data-i="i"><div class="poster skel"></div><div class="t">&nbsp;</div><div class="s">&nbsp;</div></div>
        </template>
        <template v-if="loading && !items.length"><div v-for="n in 12" :key="n" class="card ph"><div class="poster skel"></div></div></template>
      </div>
      <PickBar v-if="pickedItems.length" :items="pickedItems" @clear="clearPicks" @all="pickAll" @changed="refreshPicked" />
      <p v-if="total === 0" class="empty">{{ t('Nothing found.') }}</p>
      <div v-if="marks.length" ref="letterBox" class="letters" :class="{ scrub: !!bubble }"
        @touchstart.passive="touchStart" @touchmove="touchMove" @touchend="touchEnd" @touchcancel="scrubEnd" @contextmenu="bubble && $event.preventDefault()">
        <button v-for="m in marks" :key="m.key" :class="{ on: current === m.key }" :title="`${m.key} · ${m.size}`" @click="jump(m)">{{ m.key }}</button>
      </div>
      <div v-if="bubble" class="letterbubble" :style="{ top: bubbleTop + 'px' }" aria-hidden="true">{{ bubble }}</div>
      </template>
    </template>
  </div>
</template>
