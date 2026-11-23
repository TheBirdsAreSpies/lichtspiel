<script setup lang="ts">
// What the server does in the background, opened with a click on the bar at the top (owner), as in the classic
// app: what runs now and what comes next in it, scans, details being downloaded, what is planned afterwards,
// and the last result. Read again every 3 seconds while open; a click elsewhere closes it.
import { onBeforeUnmount, onMounted, ref } from 'vue';
import { get } from '../api';
import { t } from '../i18n';

interface Planned { n: number, mode?: string, examples: string[] }
interface Act {
  job?: { label: string, done: number, total: number, step: number, steps: number, items: string[], more: number } | null,
  scanning: string[], details: { working: string[], total: number, queued: string[], more: number },
  next: { credits: Planned, intros: Planned }, last?: { message?: string, ok?: boolean } | null,
}
const emit = defineEmits<{ close: [] }>();
const d = ref<Act | null>(null);
const box = ref<HTMLElement | null>(null);
let timer = 0;
async function load() {
  try { d.value = await get<Act>('/api/activity'); } catch { /* tried again below */ }
  timer = window.setTimeout(load, 3000);
}
const modes: Record<string, string> = { asap: 'as soon as possible', scheduled: 'once a day' };
const mode = (m?: string) => (m && modes[m] ? ' · ' + t(modes[m]) : '');
const pct = (j: NonNullable<Act['job']>) => Math.floor(100 * j.done / Math.max(1, j.total));
const head = (j: NonNullable<Act['job']>) => (j.label === 'Optimizing' ? t('{label}: {pct} %', { label: t(j.label), pct: pct(j) }) : t('{label}: {done} of {total}', { label: t(j.label), done: j.step, total: j.steps }));
// (The bar's own click toggles the panel: the panel ignores it here.)
const outside = (e: Event) => { const el = e.target as Element; if (!box.value?.contains(el) && !el.closest('.activity')) emit('close'); };
onMounted(() => { load(); document.addEventListener('pointerdown', outside, true); });
onBeforeUnmount(() => { clearTimeout(timer); document.removeEventListener('pointerdown', outside, true); });
</script>

<template>
  <div ref="box" class="actpanel" role="dialog" :aria-label="t('Running now')">
    <button class="small close" :title="t('Close')" :aria-label="t('Close')" @click="emit('close')">✕</button>
    <div v-if="!d" class="skel line"></div>
    <template v-else>
      <div v-if="d.job" class="actsec">
        <h4>{{ t('Running now') }}</h4>
        <div class="row-c"><span>{{ head(d.job) }}</span><span class="pbar"><i :style="{ width: pct(d.job) + '%' }"></i></span></div>
        <template v-if="d.job.items.length">
          <div class="dim gap">{{ t('Next up') }}</div>
          <ul><li v-for="(x, i) in d.job.items" :key="i">{{ x }}</li><li v-if="d.job.more > 0" class="dim">{{ t('… and {n} more', { n: d.job.more }) }}</li></ul>
        </template>
      </div>
      <div v-if="d.scanning.length" class="actsec">
        <h4>{{ t('Scanning') }}</h4>
        <ul><li v-for="x in d.scanning" :key="x">{{ x }}</li></ul>
      </div>
      <div v-if="d.details.working.length || d.details.total" class="actsec">
        <h4>{{ t('Downloading details') }}</h4>
        <div v-if="d.details.working.length">▶ {{ d.details.working.join(', ') }}</div>
        <template v-if="d.details.total">
          <div class="dim gap">{{ t('Waiting ({n})', { n: d.details.total }) }}</div>
          <ul><li v-for="(x, i) in d.details.queued" :key="i">{{ x }}</li><li v-if="d.details.more > 0" class="dim">{{ t('… and {n} more', { n: d.details.more }) }}</li></ul>
        </template>
      </div>
      <div class="actsec">
        <h4>{{ t('Planned afterwards') }}</h4>
        <template v-for="[p, label] in ([[d.next.credits, 'Credits detection: {n} videos waiting'], [d.next.intros, 'Intro detection: {n} seasons waiting']] as [Planned, string][])" :key="label">
          <template v-if="p.n">
            <div>{{ t(label, { n: p.n }) + mode(p.mode) }}</div>
            <ul v-if="p.examples.length"><li v-for="(x, i) in p.examples" :key="i">{{ x }}</li><li v-if="p.n > p.examples.length" class="dim">{{ t('… and {n} more', { n: p.n - p.examples.length }) }}</li></ul>
          </template>
        </template>
        <div v-if="!d.next.credits.n && !d.next.intros.n" class="dim">{{ t('Nothing else is planned.') }}</div>
      </div>
      <div v-if="d.last && d.last.message" class="actsec">
        <h4>{{ t('Last result') }}</h4>
        <div>{{ (d.last.ok === false ? '⚠ ' : '✓ ') + t(d.last.message) }}</div>
      </div>
    </template>
  </div>
</template>
