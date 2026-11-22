<script setup lang="ts">
// A poster in a row or grid: a link to the title, with how far it was watched, how many episodes are
// left, a tick once watched and a play button (for what plays itself).
import { computed } from 'vue';
import { useRouter } from 'vue-router';
import { picture, type Item } from '../api';
import { cardLines, isPlayable, isWatched, resumeAt } from '../format';
import { t } from '../i18n';
import Icon from './Icon.vue';

const props = defineProps<{ item: Item, wide?: boolean }>();
const router = useRouter();
const it = computed(() => props.item);
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
  <RouterLink :to="'/item/' + it.id" class="card" :class="{ wide, watched: isWatched(it), missing: it.missing }">
    <div class="poster">
      <img v-if="src" :src="src" :srcset="srcset" sizes="(max-width:760px) 34vw, 180px" loading="lazy" alt="" @load="($event.target as HTMLElement).classList.add('in')">
      <div v-if="progress > 0" class="bar"><i :style="{ width: progress * 100 + '%' }"></i></div>
      <span v-if="unseen > 0" class="badge">{{ unseen }}</span>
      <span v-if="isWatched(it)" class="seen" :title="t('Watched')"><Icon name="check" /></span>
      <button v-if="isPlayable(it) && !it.missing" class="play" :title="t('Play')" :aria-label="t('Play')" @click.prevent.stop="play"><Icon name="play" /></button>
    </div>
    <div class="t" :title="lines[0]">{{ lines[0] }}</div>
    <div class="s">{{ lines[1] }}</div>
  </RouterLink>
</template>
