<script setup lang="ts">
// The calendar: what comes in the next 90 days, day by day (see coming.ts).
import { computed, onMounted, ref } from 'vue';
import { picture } from '../api';
import { comingDay, comingWhat, loadComing, type Coming } from '../coming';
import { t } from '../i18n';

const DAYS = 90;
const entries = ref<Coming[] | null>(null);
const error = ref('');
onMounted(async () => {
  try { entries.value = (await loadComing(DAYS)).entries; } catch (e) { error.value = (e as Error).message; }
});
const days = computed(() => {
  const m = new Map<string, Coming[]>();
  for (const e of entries.value || []) { if (!m.has(e.date)) m.set(e.date, []); m.get(e.date)!.push(e); }
  return [...m];
});
const pic = (e: Coming) => (e.thumb ? picture(e.thumb, 100) : e.poster || '');
</script>

<template>
  <div>
    <h1>{{ t('Calendar') }}</h1>
    <p class="dim">{{ t('New episodes of the shows in your libraries, looked up once a day, and what Radarr and Sonarr expect when the wish list plugin is on.') }}</p>
    <p v-if="error" class="err">{{ error }}</p>
    <div v-else-if="entries === null" class="skel line"></div>
    <p v-else-if="!entries.length" class="empty">{{ t('Nothing is coming in the next {n} days.', { n: DAYS }) }}</p>
    <template v-for="[date, list] in days" :key="date">
      <h2>{{ comingDay(date, true) }}</h2>
      <div class="pp-items">
        <div v-for="(e, i) in list" :key="i" class="pp-item">
          <img v-if="pic(e)" :src="pic(e)" alt="" loading="lazy" referrerpolicy="no-referrer">
          <div v-else class="pp-noimg"></div>
          <div class="grow">
            <RouterLink v-if="e.show_id" :to="'/item/' + e.show_id"><b>{{ e.title }}</b></RouterLink><b v-else>{{ e.title }}</b>
            <div class="dim">{{ comingWhat(e) }}</div>
          </div>
        </div>
      </div>
    </template>
  </div>
</template>
