<script setup lang="ts">
// Your own rating: five stars, each worth 2 on the server's 0–10 scale. The left half of a star gives the
// half star, the right half the whole one.
import { ref } from 'vue';
import { api } from '../api';
import { lang, t } from '../i18n';
import { toast } from '../toast';

const props = defineProps<{ id: number, rating?: number }>();
const cur = ref(props.rating || 0);
const hover = ref<number | null>(null);
const shown = () => Math.round(hover.value ?? cur.value);
const stars = (v: number) => (v / 2).toLocaleString(lang.value, { maximumFractionDigits: 1 });
const pointsAt = (e: MouseEvent, i: number) => { const r = (e.currentTarget as HTMLElement).getBoundingClientRect(); return i * 2 - (e.clientX - r.left < r.width / 2 ? 1 : 0); };
async function save(v: number) {
  try {
    await api('POST', `/:/rate?key=${props.id}&rating=${v}`);
    cur.value = v < 0 ? 0 : v;
    toast(v < 0 ? t('Rating removed') : t('Rated {n} of 5 stars', { n: stars(v) }));
  } catch (e) { toast((e as Error).message); }
}
</script>

<template>
  <span class="stars" :title="t('Your rating')" @mouseleave="hover = null">
    <button v-for="i in 5" :key="i" type="button" :class="{ on: i * 2 <= shown(), half: i * 2 - 1 === shown() }"
      :aria-label="t('{n} stars (click the left half for {half})', { n: i, half: stars(i * 2 - 1) })" :title="t('{half} or {n} of 5', { half: stars(i * 2 - 1), n: i })"
      @mousemove="hover = pointsAt($event, i)" @click="save($event.detail === 0 ? i * 2 : pointsAt($event, i))">★</button>
    <button type="button" class="clear" :style="{ visibility: cur ? 'visible' : 'hidden' }" :title="t('Remove your rating')" @click="save(-1)">{{ t('clear') }}</button>
  </span>
</template>
