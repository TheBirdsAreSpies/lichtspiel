<script setup lang="ts">
// The home page's "Coming soon" row: one card a show (its next episode, "+2" when more follow), every movie.
import { computed, ref } from 'vue';
import { picture } from '../api';
import { comingDay, comingWhat, type Coming } from '../coming';
import { t } from '../i18n';
import Icon from './Icon.vue';

const props = defineProps<{ entries: Coming[] }>();
const cards = computed(() => {
  const seen = new Map<string, { e: Coming, more: number }>(), out: { e: Coming, more: number }[] = [];
  for (const e of props.entries) {
    const k = e.show_id ? 's' + e.show_id : e.kind === 'episode' ? 'e' + e.title : null;
    if (k && seen.has(k)) { seen.get(k)!.more++; continue; }
    const c = { e, more: 0 };
    if (k) seen.set(k, c);
    out.push(c);
  }
  return out;
});
const pic = (e: Coming) => (e.thumb ? picture(e.thumb, 360) : e.poster || '');
const line = (e: Coming) => e.title + (comingWhat(e) ? ' · ' + comingWhat(e) : '');
const strip = ref<HTMLElement | null>(null);
const page = (dir: number) => { const el = strip.value; if (el) el.scrollBy({ left: dir * el.clientWidth * 0.8, behavior: 'smooth' }); };
</script>

<template>
  <section v-if="cards.length" class="row">
    <div class="row-head">
      <h2>{{ t('Coming Soon') }} <RouterLink to="/calendar" class="dim row-more">{{ t('Calendar') }} ›</RouterLink></h2>
      <span class="row-arrows">
        <button class="iconbtn" :aria-label="t('Back')" @click="page(-1)"><Icon name="left" /></button>
        <button class="iconbtn" :aria-label="t('Next')" @click="page(1)"><Icon name="right" /></button>
      </span>
    </div>
    <div ref="strip" class="strip">
      <component :is="e.show_id ? 'RouterLink' : 'div'" v-for="({ e, more }, i) in cards" :key="i" :to="e.show_id ? '/item/' + e.show_id : undefined" class="card" :title="line(e)">
        <div class="poster">
          <img v-if="pic(e)" :src="pic(e)" loading="lazy" alt="" referrerpolicy="no-referrer" @load="($event.target as HTMLElement).classList.add('in')">
          <span v-if="more" class="badge">+{{ more }}</span>
        </div>
        <div class="t">{{ comingDay(e.date) }}</div>
        <div class="s">{{ line(e) }}</div>
      </component>
    </div>
  </section>
</template>
