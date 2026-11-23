<script setup lang="ts">
// Weekdays (rows, Monday first) by hours (columns): the stronger the colour, the more. A tooltip shows a cell.
import { computed, onBeforeUnmount, onMounted, ref } from 'vue';
import { lang, t } from '../i18n';

const props = defineProps<{ grid: number[][], fmt: (v: number, full?: boolean) => string }>();
const box = ref<HTMLElement | null>(null);
const width = ref(700);
let ro: ResizeObserver | null = null;
onMounted(() => { ro = new ResizeObserver(() => { if (box.value) width.value = Math.max(320, box.value.clientWidth); }); if (box.value) ro.observe(box.value); });
onBeforeUnmount(() => ro && ro.disconnect());

const L = 44, T = 4, B = 22;
const cw = computed(() => (width.value - L) / 24);
const ch = computed(() => Math.min(28, Math.max(16, cw.value * 0.8)));
const H = computed(() => T + ch.value * 7 + B);
const max = computed(() => Math.max(...props.grid.flat(), 0));
// (7 January 2024 was a Sunday.)
const names = computed(() => [1, 2, 3, 4, 5, 6, 0].map(d => new Date(2024, 0, 7 + d).toLocaleDateString(lang.value, { weekday: 'short' })));
const hourTicks = computed(() => Array.from({ length: 24 }, (_, h) => h).filter(h => h % (cw.value < 22 ? 3 : 2) === 0));
const two = (n: number) => String(n).padStart(2, '0');

const hov = ref<[number, number] | null>(null);
const tip = ref<{ x: number, y: number } | null>(null);
const tipStyle = computed(() => {
  const p = tip.value; if (!p) return {};
  const left = p.x + 180 > innerWidth - 8 ? p.x - 194 : p.x + 14, top = p.y + 80 > innerHeight - 8 ? p.y - 80 : p.y + 14;
  return { left: Math.max(8, left) + 'px', top: Math.max(8, top) + 'px' };
});
</script>

<template>
  <div ref="box" class="chart">
    <svg :viewBox="`0 0 ${width} ${H}`" width="100%" :height="H" class="viz" role="img" :aria-label="t('Times of day')">
      <g v-for="(row, d) in grid" :key="d">
        <text :x="L - 8" :y="T + ch * d + ch / 2 + 4" text-anchor="end" class="tick">{{ names[d] }}</text>
        <rect v-for="(v, h) in row" :key="h" :x="L + cw * h + 1" :y="T + ch * d + 1" :width="Math.max(1, cw - 2)" :height="ch - 2" rx="3"
          :fill="v > 0 ? 'var(--s1)' : 'var(--panel2)'" :fill-opacity="v > 0 ? 0.18 + 0.82 * v / max : 1"
          @mousemove="hov = [d, h]; tip = { x: $event.clientX, y: $event.clientY }" @mouseleave="hov = null; tip = null" />
      </g>
      <text v-for="h in hourTicks" :key="'h' + h" :x="L + cw * h + cw / 2" :y="H - 6" text-anchor="middle" class="tick">{{ h }}</text>
    </svg>
    <Teleport to="body">
      <div v-if="tip && hov" class="viztip" :style="tipStyle" role="tooltip">
        <div class="tl"><span>{{ names[hov[0]] }} {{ two(hov[1]) }}–{{ two((hov[1] + 1) % 24) }}</span></div>
        <div class="tl"><i style="background: var(--s1)"></i><span></span><b>{{ fmt(grid[hov[0]][hov[1]], true) }}</b></div>
      </div>
    </Teleport>
  </div>
</template>
