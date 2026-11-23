<script setup lang="ts">
// "Something short": say how much time there is, and get what fits into it. From home: what you are watching
// (titles begun count with the time left) and your watchlist (GET /api/short). From a library: what in it
// fits and is not watched yet, in random order (a show library: its episodes).
import { ref } from 'vue';
import { get, list, type Item, type Section } from '../api';
import { fmtDur } from '../format';
import { t } from '../i18n';
import Modal from './Modal.vue';
import PosterCard from './PosterCard.vue';

const props = defineProps<{ section?: Section }>();
const emit = defineEmits<{ close: [] }>();
const TIMES = [20, 30, 45, 60, 90];
const kept = (() => { try { return Number(localStorage.getItem('shortMinutes')) || 30; } catch { return 30; } })();
const minutes = ref(kept);
const own = ref(TIMES.includes(kept) ? '' : String(kept));
const items = ref<Item[] | null>(null);
const error = ref('');
let asked = 0;
const left = (it: Item) => Math.max(0, (it.duration || 0) - (it.viewOffset || 0));

async function choose(m: number) {
  minutes.value = m; own.value = TIMES.includes(m) ? '' : String(m);
  try { localStorage.setItem('shortMinutes', String(m)); } catch { /* none */ }
  const mine = ++asked;
  items.value = null; error.value = '';
  try {
    let url = `/api/short?minutes=${m}`;
    if (props.section) {
      const p = new URLSearchParams({ sort: 'random:asc', start: '0', size: '30', 'duration>': '1', 'duration<': String(m) });
      if (props.section.type === 'show') { p.set('type', 'episode'); p.set('unwatched', 'true'); } else p.set('unwatched', 'true');
      url = `/library/sections/${props.section.id}/all?${p}`;
    }
    const got = list<Item>(await get(url));
    if (mine === asked) items.value = got;
  } catch (e) { if (mine === asked) error.value = (e as Error).message; }
}
function typed() { const v = Math.round(Number(own.value)); if (v > 0) choose(Math.min(600, v)); }
choose(kept);
</script>

<template>
  <Modal :title="t('Something short')" wide @close="emit('close')">
    <p class="dim">{{ section ? t('How much time do you have? What fits comes from “{name}”, not watched yet, in random order.', { name: section.title }) : t('How much time do you have? What fits comes from what you are watching and from your watchlist.') }}</p>
    <div class="picktimes">
      <button v-for="v in TIMES" :key="v" class="small" :class="{ primary: v === minutes }" :aria-pressed="v === minutes" @click="choose(v)">{{ t('{m} min', { m: v }) }}</button>
      <label><input v-model="own" type="number" min="1" max="600" :aria-label="t('Minutes')" @change="typed"> {{ t('min') }}</label>
    </div>
    <p v-if="error" class="err">{{ error }}</p>
    <div v-else-if="items === null" class="dim">{{ t('Loading…') }}</div>
    <div v-else-if="items.length" class="picklist" @click.capture="($event.target as Element).closest('a') && !($event.target as Element).closest('.more, .edit, .pick') && emit('close')">
      <div v-for="it in items" :key="it.id">
        <PosterCard :item="it" plain />
        <div class="dim small">{{ it.viewOffset ? t('{time} left', { time: fmtDur(left(it)) }) : fmtDur(it.duration) }}</div>
      </div>
    </div>
    <p v-else class="dim">{{ section ? t('Nothing in “{name}” that you have not watched fits into {m} minutes.', { name: section.title, m: minutes }) : t('Nothing you are watching or have on your watchlist fits into {m} minutes.', { m: minutes }) }}</p>
    <template #actions><button @click="emit('close')">{{ t('Close') }}</button></template>
  </Modal>
</template>
