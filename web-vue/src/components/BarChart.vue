<script setup lang="ts">
// Stacked bars per day or week (as in the classic dashboard): buckets with their parts, keys = [key, label,
// colour]. A tooltip shows a bar's parts and their sum.
import { computed, onBeforeUnmount, onMounted, ref } from 'vue';
import { niceTop } from '../format';
import { t } from '../i18n';

export interface Bucket { label: string, tip: string, parts: Record<string, number> }
const props = withDefaults(defineProps<{ buckets: Bucket[], keys: [string, string, string][], fmt: (v: number, full?: boolean) => string, height?: number }>(), { height: 220 });

const box = ref<HTMLElement | null>(null);
const width = ref(700);
let ro: ResizeObserver | null = null;
onMounted(() => { ro = new ResizeObserver(() => { if (box.value) width.value = Math.max(280, box.value.clientWidth); }); if (box.value) ro.observe(box.value); });
onBeforeUnmount(() => ro && ro.disconnect());

const L = 44, R = 8, T = 10, B = 24;
const W = computed(() => width.value), H = computed(() => props.height);
const totals = computed(() => props.buckets.map(b => props.keys.reduce((a, [k]) => a + (b.parts[k] || 0), 0)));
const yMax = computed(() => niceTop(Math.max(...totals.value, 0) || 1));
const n = computed(() => props.buckets.length);
const slot = computed(() => (W.value - L - R) / Math.max(1, n.value));
const bw = computed(() => Math.max(2, Math.min(28, slot.value * 0.72)));
const Y = (v: number) => T + (1 - v / yMax.value) * (H.value - T - B);
const grid = computed(() => [0, 1, 2, 3, 4].map(k => { const v = (yMax.value * k) / 4; return { k, y: Y(v), label: props.fmt(v) }; }));
const every = computed(() => Math.ceil(n.value / Math.max(1, Math.floor((W.value - L - R) / 64))));
// Each bar's parts as paths: a 2 px gap between them, the top one with rounded corners.
const bars = computed(() => props.buckets.map((b, i) => {
  const cx = L + slot.value * i + slot.value / 2, x = cx - bw.value / 2, w = bw.value;
  const segs = props.keys.filter(([k]) => b.parts[k] > 0);
  let base = 0;
  const parts = segs.map(([k, , color], si) => {
    const v = b.parts[k], y0 = Y(base), y1 = Y(base + v);
    const top = si === segs.length - 1, gap = si > 0 ? 2 : 0, hgt = Math.max(1, y0 - y1 - gap), r = top ? Math.min(4, w / 2, hgt) : 0;
    const yt = y1, yb = y1 + hgt;
    base += v;
    return { d: `M${x} ${yb}V${yt + r}Q${x} ${yt} ${x + r} ${yt}H${x + w - r}Q${x + w} ${yt} ${x + w} ${yt + r}V${yb}Z`, color };
  });
  return { cx, parts, label: i % every.value === 0 ? b.label : '' };
}));

const hov = ref<number | null>(null);
const tip = ref<{ x: number, y: number } | null>(null);
const tipStyle = computed(() => {
  const p = tip.value; if (!p) return {};
  const left = p.x + 180 > innerWidth - 8 ? p.x - 194 : p.x + 14, top = p.y + 130 > innerHeight - 8 ? p.y - 130 : p.y + 14;
  return { left: Math.max(8, left) + 'px', top: Math.max(8, top) + 'px' };
});
</script>

<template>
  <div ref="box" class="chart">
    <svg :viewBox="`0 0 ${W} ${H}`" width="100%" :height="H" class="viz" role="img">
      <template v-for="g in grid" :key="g.k">
        <line :x1="L" :x2="W - R" :y1="g.y" :y2="g.y" :class="g.k ? 'grid' : 'axis'" />
        <text :x="L - 6" :y="g.y + 4" text-anchor="end" class="tick">{{ g.label }}</text>
      </template>
      <g v-for="(b, i) in bars" :key="i">
        <text v-if="b.label" :x="b.cx" :y="H - 7" text-anchor="middle" class="tick">{{ b.label }}</text>
        <path v-for="(p, j) in b.parts" :key="j" :d="p.d" :fill="p.color" />
        <rect :x="L + slot * i" :y="T" :width="slot" :height="H - T - B" fill="transparent" :class="{ hov: hov === i }"
          @mousemove="hov = i; tip = { x: $event.clientX, y: $event.clientY }" @mouseleave="hov = null; tip = null" />
      </g>
    </svg>
    <div class="legend"><span v-for="[k, l, c] in keys" :key="k"><i :style="{ background: c }"></i>{{ t(l) }}</span></div>
    <Teleport to="body">
      <div v-if="tip && hov !== null" class="viztip" :style="tipStyle" role="tooltip">
        <div class="tl"><span>{{ buckets[hov].tip }}</span></div>
        <div v-for="[k, l, c] in keys" :key="k" class="tl"><i :style="{ background: c }"></i><span>{{ t(l) }}</span><b>{{ fmt(buckets[hov].parts[k] || 0, true) }}</b></div>
        <div class="tl"><span>{{ t('Together') }}</span><b>{{ fmt(totals[hov], true) }}</b></div>
      </div>
    </Teleport>
  </div>
</template>
