<script setup lang="ts">
// Lines over time (as in the classic dashboard): series of values at the same times (unix seconds). A gap
// (no value: the server wasn't running) breaks a line; `fill` shades below it. A crosshair and a tooltip
// follow the pointer. `dates`: the range is days long (the axis shows dates).
import { computed, onBeforeUnmount, onMounted, ref } from 'vue';
import { niceTop } from '../format';
import { lang, t } from '../i18n';

export interface Series { name: string, color: string, values: (number | null | undefined)[], fill?: boolean }
const props = withDefaults(defineProps<{ times: number[], series: Series[], max?: number | null, fmt: (v: number, full?: boolean) => string, height?: number, dates?: boolean }>(), { height: 150, max: null });

const box = ref<HTMLElement | null>(null);
const width = ref(600);
let ro: ResizeObserver | null = null;
onMounted(() => { ro = new ResizeObserver(() => { if (box.value) width.value = Math.max(260, box.value.clientWidth); }); if (box.value) ro.observe(box.value); });
onBeforeUnmount(() => ro && ro.disconnect());

const L = 52, R = 10, T = 10, B = 22;
const W = computed(() => width.value);
const H = computed(() => props.height);
const n = computed(() => props.times.length);
const yMax = computed(() => {
  const all = props.series.flatMap(s => s.values).filter((v): v is number => v !== null && v !== undefined);
  return niceTop(props.max || Math.max(...all, 0) * 1.1 || 1);
});
const X = (i: number) => L + (n.value <= 1 ? 0 : (i / (n.value - 1)) * (W.value - L - R));
const Y = (v: number) => T + (1 - v / yMax.value) * (H.value - T - B);
const when = (sec: number, tip: boolean) => {
  const d = new Date(sec * 1000);
  if (props.dates) return tip ? d.toLocaleString(lang.value, { day: 'numeric', month: 'numeric', year: 'numeric', hour: '2-digit', minute: '2-digit' }) : d.toLocaleDateString(lang.value, { day: 'numeric', month: 'numeric' });
  return tip ? d.toLocaleTimeString(lang.value) : d.toLocaleTimeString(lang.value, { hour: '2-digit', minute: '2-digit' });
};
const grid = computed(() => [0, 1, 2, 3, 4].map(k => { const v = (yMax.value * k) / 4; return { k, y: Y(v), label: props.fmt(v) }; }));
const xLabels = computed(() => {
  const out: { x: number, label: string, anchor: string }[] = [];
  const count = Math.min(5, n.value);
  let prev = '';
  for (let k = 0; k < count; k++) {
    const i = Math.round((k / Math.max(1, count - 1)) * (n.value - 1)), label = when(props.times[i], false);
    if (label === prev) continue;
    prev = label;
    out.push({ x: X(i), label, anchor: k === 0 ? 'start' : k === count - 1 ? 'end' : 'middle' });
  }
  return out;
});
// Each series: its line, and (with fill) one shaded piece per run of values.
const paths = computed(() => props.series.map(s => {
  let d = '', area = '', seg = '', started = false, first = 0, last = 0;
  const close = () => { if (seg) area += seg + `L${X(last).toFixed(1)} ${Y(0)}L${X(first).toFixed(1)} ${Y(0)}Z`; seg = ''; };
  s.values.forEach((v, i) => {
    if (v === null || v === undefined) { if (started) close(); started = false; return; }
    const p = (started ? 'L' : 'M') + X(i).toFixed(1) + ' ' + Y(v).toFixed(1);
    d += p; seg += p;
    if (!started) first = i;
    started = true; last = i;
  });
  close();
  return { d, area: s.fill ? area : '', color: s.color };
}));

// The pointer: the nearest time, its values in the tooltip.
const at = ref<number | null>(null);
const tip = ref<{ x: number, y: number } | null>(null);
function move(e: MouseEvent) {
  if (!n.value) return;
  const r = (e.currentTarget as SVGElement).ownerSVGElement!.getBoundingClientRect(), px = ((e.clientX - r.left) / r.width) * W.value;
  at.value = Math.max(0, Math.min(n.value - 1, Math.round(((px - L) / (W.value - L - R)) * (n.value - 1))));
  tip.value = { x: e.clientX, y: e.clientY };
}
function leave() { at.value = null; tip.value = null; }
const tipStyle = computed(() => {
  const p = tip.value; if (!p) return {};
  const left = p.x + 180 > innerWidth - 8 ? p.x - 194 : p.x + 14, top = p.y + 120 > innerHeight - 8 ? p.y - 120 : p.y + 14;
  return { left: Math.max(8, left) + 'px', top: Math.max(8, top) + 'px' };
});
const shown = (v: number | null | undefined) => (v === null || v === undefined ? '–' : props.fmt(v, true));
</script>

<template>
  <div ref="box" class="chart">
    <svg :viewBox="`0 0 ${W} ${H}`" width="100%" :height="H" class="viz" role="img">
      <template v-for="g in grid" :key="g.k">
        <line :x1="L" :x2="W - R" :y1="g.y" :y2="g.y" :class="g.k ? 'grid' : 'axis'" />
        <text :x="L - 6" :y="g.y + 4" text-anchor="end" class="tick">{{ g.label }}</text>
      </template>
      <text v-for="l in xLabels" :key="l.label + l.x" :x="l.x" :y="H - 6" :text-anchor="l.anchor" class="tick">{{ l.label }}</text>
      <template v-for="(p, i) in paths" :key="i">
        <path v-if="p.area" :d="p.area" :fill="p.color" opacity="0.14" />
        <path v-if="p.d" :d="p.d" fill="none" :stroke="p.color" stroke-width="2" stroke-linejoin="round" stroke-linecap="round" />
      </template>
      <text v-if="!n" :x="W / 2" :y="H / 2" text-anchor="middle" class="tick">{{ t('Collecting values…') }}</text>
      <template v-if="at !== null">
        <line :x1="X(at)" :x2="X(at)" :y1="T" :y2="H - B" class="cross" />
        <template v-for="(s, i) in series" :key="'d' + i">
          <circle v-if="s.values[at] !== null && s.values[at] !== undefined" r="4" :cx="X(at)" :cy="Y(s.values[at] as number)" :fill="s.color" stroke="var(--panel)" stroke-width="2" />
        </template>
      </template>
      <rect :x="L" :y="T" :width="Math.max(0, W - L - R)" :height="Math.max(0, H - T - B)" fill="transparent" @mousemove="move" @mouseleave="leave" />
    </svg>
    <div v-if="series.length > 1" class="legend"><span v-for="s in series" :key="s.name"><i :style="{ background: s.color }"></i>{{ s.name }}</span></div>
    <Teleport to="body">
      <div v-if="tip && at !== null" class="viztip" :style="tipStyle" role="tooltip">
        <div class="tl"><span>{{ when(times[at], true) }}</span></div>
        <div v-for="s in series" :key="s.name" class="tl"><i :style="{ background: s.color }"></i><span>{{ s.name }}</span><b>{{ shown(s.values[at]) }}</b></div>
      </div>
    </Teleport>
  </div>
</template>
