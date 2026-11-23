<script setup lang="ts">
// A poster in a row or grid: a link to the title, with how far it was watched, how many episodes are
// left, a tick once watched and a play button (for what plays itself).
import { computed, ref, watch } from 'vue';
import { useRouter } from 'vue-router';
import { get, list, picture, session, type Item } from '../api';
import { cardLines, isMusic, isPlayable, resumeAt, showSeen } from '../format';
import { t } from '../i18n';
import Icon from './Icon.vue';
import EditDetails from './EditDetails.vue';
import ItemMenu from './ItemMenu.vue';

// pickable: a circle on the poster selects it (so do Ctrl and Shift with a click, and on a touch screen a tap
// while something is selected); the page keeps what is selected and says so with picked.
// plain: without the pencil and the "⋯" (a poster inside a box).
const props = defineProps<{ item: Item, wide?: boolean, pickable?: boolean, picked?: boolean, picking?: boolean, plain?: boolean, cont?: boolean }>();
const emit = defineEmits<{ pick: [e: MouseEvent], changed: [id: number], removed: [id: number] }>();
// After its menu changed something (watched, watchlist, edited): the title as the server has it now.
const fresh = ref<Item | null>(null);
watch(() => props.item, () => { fresh.value = null; });
async function changed() {
  try { const f = list<Item>(await get('/library/metadata/' + props.item.id))[0]; if (f) fresh.value = { ...props.item, ...f }; } catch { /* keep it */ }
  emit('changed', props.item.id);
}
const editing = ref(false);
const selectMe = () => emit('pick', new MouseEvent('click'));
const admin = computed(() => !!session.me?.admin && !props.plain && props.item.type !== 'clip');
function click(e: MouseEvent) {
  if (!props.pickable) return;
  const touchPicking = props.picking && matchMedia('(hover: none)').matches;
  if (!touchPicking && !e.ctrlKey && !e.metaKey && !e.shiftKey) return;
  e.preventDefault(); e.stopPropagation(); emit('pick', e);
}
const router = useRouter();
const it = computed(() => fresh.value || props.item);
const lines = computed(() => cardLines(it.value));
const progress = computed(() => (it.value.viewOffset && it.value.duration ? Math.min(1, it.value.viewOffset / it.value.duration) : 0));
const unseen = computed(() => (it.value.leafCount ? it.value.leafCount - (it.value.viewedLeafCount || 0) : 0));
// An episode shows its show's poster in a row of posters; in a wide row its own still.
const img = computed(() => (it.value.type === 'episode' && it.value.grandparentThumb && !props.wide ? it.value.grandparentThumb : it.value.thumb));
const src = computed(() => picture(img.value, props.wide ? 480 : 360));
const srcset = computed(() => (img.value && !props.wide ? [240, 360, 600].map(w => `${picture(img.value, w)} ${w}w`).join(', ') : undefined));
function play() {
  router.push({ name: 'play', params: { id: it.value.id }, query: { t: String(Math.floor(resumeAt(it.value))) } });
}
</script>

<template>
  <RouterLink :to="'/item/' + it.id" class="card" :class="{ wide, music: isMusic(it), watched: showSeen(it), missing: it.missing, picked }" @click.capture="click">
    <div class="poster">
      <img v-if="src" :src="src" :srcset="srcset" sizes="(max-width:760px) 34vw, 180px" loading="lazy" alt="" @load="($event.target as HTMLElement).classList.add('in')">
      <div v-if="progress > 0" class="bar"><i :style="{ width: progress * 100 + '%' }"></i></div>
      <span v-if="unseen > 0" class="badge">{{ unseen }}</span>
      <span v-if="it.missing" class="gone" :title="t('The file is gone from disk; the item is in the library’s trash')" :aria-label="t('Unavailable')"><Icon name="close" /></span>
      <span v-if="pickable" class="pick" role="checkbox" :aria-checked="!!picked" :title="t('Select')" :aria-label="t('Select')" @click.prevent.stop="emit('pick', $event)"></span>
      <span v-if="showSeen(it)" class="seen" :title="t('Watched')"><Icon name="check" /></span>
      <button v-if="admin" type="button" class="edit" :title="t('Edit details')" :aria-label="t('Edit details')" @click.prevent.stop="editing = true"><Icon name="edit" /></button>
      <ItemMenu v-if="!plain && it.type !== 'clip'" :item="it" card :selectable="pickable" :cont="cont" @changed="changed" @removed="emit('removed', props.item.id)" @select="selectMe" />
      <button v-if="isPlayable(it) && !it.missing" class="play" :title="t('Play')" :aria-label="t('Play')" @click.prevent.stop="play"><Icon name="play" /></button>
    </div>
    <div class="t" :title="lines[0]">{{ lines[0] }}</div>
    <div class="s">{{ lines[1] }}</div>
    <EditDetails v-if="editing" :id="it.id" @close="editing = false" @saved="changed" />
  </RouterLink>
</template>
