<script setup lang="ts">
// The "Filter" button of a library and its panel: the kinds of filter on the left (a state such as
// "Unwatched" switches at once), the values of the chosen kind on the right, with a search box.
// Emits set(filter, value); an empty value takes the filter off.
import { computed, onBeforeUnmount, ref, watch } from 'vue';
import { t } from '../i18n';
import { filterName, loadFilters, MAIN, STATE, valueName, type Filter } from '../filters';
import Icon from './Icon.vue';

const props = defineProps<{ url: string, active: Record<string, string>, rememberKey: string }>();
const emit = defineEmits<{ set: [string, string] }>();
const open = ref(false);
const filters = ref<Filter[] | null>(null);
const error = ref('');
const cur = ref((() => { try { return localStorage.getItem(props.rememberKey) || ''; } catch { return ''; } })());
const needle = ref('');
const root = ref<HTMLElement | null>(null);
const LIMIT = 300;

const n = computed(() => Object.keys(props.active).length);
async function toggle() {
  open.value = !open.value;
  if (!open.value) return;
  error.value = '';
  try { filters.value = await loadFilters(props.url); } catch (e) { error.value = (e as Error).message; }
}
// A new address (another kind in a show library) asks again.
watch(() => props.url, () => { filters.value = null; if (open.value) { open.value = false; toggle(); } });

const byName = computed(() => Object.fromEntries((filters.value || []).map(f => [f.filter, f])));
const groups = computed(() => {
  const all = filters.value || [];
  const states = STATE.map(k => byName.value[k]).filter(Boolean);
  const main = MAIN.map(k => byName.value[k]).filter(Boolean);
  return [states, main, all.filter(f => !states.includes(f) && !main.includes(f) && !f.advanced)].filter(g => g.length);
});
const chosen = computed<Filter | null>(() => {
  const f = byName.value[cur.value];
  if (f && f.type !== 'boolean') return f;
  return groups.value.flat().find(x => x.type !== 'boolean') || null;
});
const shown = computed(() => {
  const f = chosen.value; if (!f) return { vals: [], more: 0 };
  const w = needle.value.trim().toLowerCase();
  const all = f.values.filter(x => !w || valueName(f.filter, x.value).toLowerCase().includes(w));
  return { vals: all.slice(0, LIMIT), more: Math.max(0, all.length - LIMIT) };
});
function pick(f: Filter) {
  if (f.type === 'boolean') { emit('set', f.filter, props.active[f.filter] ? '' : 'true'); return; }
  cur.value = f.filter; needle.value = '';
  try { localStorage.setItem(props.rememberKey, f.filter); } catch { /* none */ }
}
function choose(f: Filter, v: string) { emit('set', f.filter, props.active[f.filter] === v ? '' : v); }

// A click outside or Escape closes it.
const outside = (e: Event) => { if (open.value && root.value && !root.value.contains(e.target as Node)) open.value = false; };
const esc = (e: KeyboardEvent) => { if (open.value && e.key === 'Escape') { e.stopPropagation(); open.value = false; } };
document.addEventListener('pointerdown', outside, true);
document.addEventListener('keydown', esc, true);
onBeforeUnmount(() => { document.removeEventListener('pointerdown', outside, true); document.removeEventListener('keydown', esc, true); });
</script>

<template>
  <div ref="root" class="fbtn-wrap">
    <button class="fbtn" :class="{ on: n }" aria-haspopup="true" :aria-expanded="open" @click="toggle">
      <Icon name="filter" /><span>{{ n ? t('Filter ({n})', { n }) : t('Filter') }}</span>
    </button>
    <div v-if="open" class="fpanel" role="dialog" :aria-label="t('Filter')">
      <div class="fp-head"><b>{{ t('Filter') }}</b><button class="iconbtn" :aria-label="t('Close')" @click="open = false"><Icon name="close" /></button></div>
      <p v-if="error" class="err pad">{{ error }}</p>
      <div v-else-if="!filters" class="dim pad">{{ t('Loading…') }}</div>
      <template v-else>
        <div class="fp-left">
          <template v-for="(g, gi) in groups" :key="gi">
            <hr v-if="gi">
            <button v-for="f in g" :key="f.filter" class="fp-item" :class="{ cur: chosen && chosen.filter === f.filter, on: active[f.filter] }" @click="pick(f)">
              <span>{{ filterName(f.filter) }}</span>
              <small class="dim">{{ f.type === 'boolean' ? (active[f.filter] ? '✓' : f.count ?? '') : active[f.filter] ? valueName(f.filter, active[f.filter]).slice(0, 14) : '›' }}</small>
            </button>
          </template>
        </div>
        <div class="fp-right">
          <template v-if="chosen">
            <input v-model="needle" type="search" class="full" :placeholder="t('Search {name}…', { name: filterName(chosen.filter) })">
            <div class="fp-values">
              <button v-for="x in shown.vals" :key="x.value" class="fp-val" :class="{ on: active[chosen.filter] === x.value }" @click="choose(chosen, x.value)">
                <span>{{ valueName(chosen.filter, x.value) }}</span><small class="dim">{{ x.count }}</small>
              </button>
              <div v-if="!shown.vals.length" class="dim pad">{{ t('None found') }}</div>
              <div v-if="shown.more" class="dim pad">{{ t('{n} more — type to narrow down', { n: shown.more }) }}</div>
            </div>
          </template>
          <div v-else class="dim pad">{{ t('None found') }}</div>
        </div>
      </template>
    </div>
  </div>
</template>
