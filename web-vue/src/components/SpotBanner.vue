<script setup lang="ts">
// The home page's banner, as spotlight() in the classic app: a few titles in turn, big, over their
// backdrop. Each stays about nine seconds (the bar in its dot shows how long; the mouse on the banner
// holds it), a dot or a swipe picks another. Nothing turns by itself when the system asks for less motion.
import { computed, ref, watch } from 'vue';
import { useRouter } from 'vue-router';
import { get, list, picture, type Item } from '../api';
import { fmtDur, fmtTime, isPlayable, resumeAt, seasonTitle } from '../format';
import { t } from '../i18n';

export interface Spot { it: Item, label: string }
const props = defineProps<{ spots: Spot[] }>();
const router = useRouter();

const cur = ref(0);
// Backdrops already asked for: this one and the next, so a turn shows a loaded picture.
const loaded = ref<Set<number>>(new Set());
const ready = ref<Set<number>>(new Set());
const calm = typeof matchMedia === 'function' && matchMedia('(prefers-reduced-motion: reduce)').matches;
const turning = computed(() => props.spots.length > 1 && !calm);
// A new list (a scan brought new titles) starts at the first; the same keeps its place.
watch(() => props.spots.map(s => s.it.id).join(','), () => { cur.value = 0; loaded.value = new Set(); ready.value = new Set(); go(0); });

const artId = (it: Item) => it.grandparentRatingKey || (it.type === 'season' ? it.parentRatingKey : null) || it.id;
const art = (i: number) => { const it = props.spots[i].it; return picture(`/library/metadata/${artId(it)}/art?v=${it.updatedAt || 0}`); };

// The title's logo, when it has one, instead of its name (asked for once per title).
const logos = ref<Record<number, string>>({});
async function logoFor(it: Item) {
  if (it.type === 'episode' || it.type === 'season' || it.id in logos.value) return;
  logos.value[it.id] = '';
  const d = list<Item & { clearLogo?: string }>(await get(`/library/metadata/${it.id}`).catch(() => null))[0];
  if (d?.clearLogo) logos.value[it.id] = picture(d.clearLogo);
}

function go(i: number) {
  const n = props.spots.length;
  if (!n) return;
  i = (i + n) % n;
  cur.value = i;
  loaded.value = new Set([...loaded.value, i, (i + 1) % n]);
  logoFor(props.spots[i].it);
}
go(0);

const spot = computed(() => props.spots[cur.value]);
const show = computed(() => ['episode', 'season'].includes(spot.value.it.type));
const title = computed(() => { const it = spot.value.it; return it.type === 'episode' ? it.grandparentTitle : it.type === 'season' ? it.parentTitle : it.title; });
const facts = computed(() => {
  const it = spot.value.it;
  const sub = it.type === 'episode' ? `${t('S{n}', { n: it.parentIndex || 0 })} ${t('E{n}', { n: it.index || 0 })} · ${it.title}` : it.type === 'season' ? seasonTitle(it.title) : '';
  const rest = [it.year, it.duration && it.type !== 'show' ? fmtDur(it.duration) : '', ...(it.genres || []).slice(0, 2)].filter(Boolean).join(' · ');
  return [sub, rest].filter(Boolean).join(' · ');
});
const at = computed(() => resumeAt(spot.value.it));
const playable = computed(() => isPlayable(spot.value.it) && !spot.value.it.missing);
const logo = computed(() => (show.value ? '' : logos.value[spot.value.it.id] || ''));
const logoBroken = ref(false);
watch(cur, () => { logoBroken.value = false; });

function play() { const it = spot.value.it; router.push({ name: 'play', params: { id: it.id }, query: { t: String(Math.floor(at.value)) } }); }
// A show or season: its next episode not watched yet (or the first).
async function playNext() {
  const eps = list<Item>(await get(`/library/metadata/${spot.value.it.id}/allLeaves`).catch(() => null));
  const next = eps.find(e => (e.viewOffset || 0) > 0 && !e.viewCount) || eps.find(e => !e.viewCount) || eps[0];
  if (next) router.push({ name: 'play', params: { id: next.id }, query: { t: String(Math.floor(resumeAt(next))) } });
}

// The running bar in the dot ends → the next title.
function ran(e: AnimationEvent, k: number) { if (e.animationName === 'spot-run' && k === cur.value) go(cur.value + 1); }
let x0: number | null = null;
function down(e: PointerEvent) { if (e.pointerType === 'touch') x0 = e.clientX; }
function up(e: PointerEvent) {
  if (x0 === null) return;
  const dx = e.clientX - x0; x0 = null;
  if (Math.abs(dx) > 50 && props.spots.length > 1) go(cur.value + (dx < 0 ? 1 : -1));
}
</script>

<template>
  <section class="spot" :class="{ turning }" aria-roledescription="carousel" @pointerdown="down" @pointerup="up" @pointercancel="x0 = null">
    <div class="spot-bg">
      <template v-for="(s, i) in spots" :key="s.it.id">
        <img v-if="loaded.has(i)" :src="art(i)" alt="" :class="{ on: i === cur, ok: ready.has(i) }" @load="ready = new Set([...ready, i])">
      </template>
    </div>
    <div :key="cur" class="spot-body">
      <div v-if="spot.label" class="spot-label">{{ spot.label }}</div>
      <h2 class="spot-title"><img v-if="logo && !logoBroken" :src="logo" :alt="title" @error="logoBroken = true"><template v-else>{{ title }}</template></h2>
      <div v-if="facts" class="spot-facts">{{ facts }}</div>
      <p v-if="spot.it.summary" class="spot-sum">{{ spot.it.summary }}</p>
      <div class="spot-act">
        <button v-if="playable" class="primary big" @click="play">{{ at ? t('▶ Resume from {time}', { time: fmtTime(at) }) : t('▶ Play') }}</button>
        <button v-else-if="(spot.it.type === 'show' || spot.it.type === 'season') && spot.it.leafCount" class="primary big" @click="playNext">{{ t('▶ Play next episode') }}</button>
        <RouterLink class="spot-more" :to="'/item/' + spot.it.id">{{ t('More info') }}</RouterLink>
      </div>
    </div>
    <div v-if="spots.length > 1" class="spot-dots">
      <!-- (The button is a bigger, invisible place to tap; the dot is the <i> in it.) -->
      <button v-for="(s, k) in spots" :key="s.it.id" type="button" :class="{ on: k === cur }" :aria-current="k === cur" :title="s.it.grandparentTitle || s.it.title"
        :aria-label="s.it.grandparentTitle || s.it.title" @click="go(k)"><i @animationend="ran($event, k)"></i></button>
    </div>
  </section>
</template>
