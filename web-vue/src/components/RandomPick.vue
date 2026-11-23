<script setup lang="ts">
// The dice of a library: the title picked, with its description, to play, to open, or to throw again.
import { computed } from 'vue';
import { useRouter } from 'vue-router';
import { get, list, type Item } from '../api';
import { fmtDur, fmtTime, isPlayable, resumeAt, seasonTitle } from '../format';
import { t } from '../i18n';
import Modal from './Modal.vue';
import PosterCard from './PosterCard.vue';

const props = defineProps<{ item: Item }>();
const emit = defineEmits<{ close: [], again: [] }>();
const router = useRouter();
const it = computed(() => props.item);
const name = computed(() => (it.value.type === 'episode' ? `${it.value.grandparentTitle} · ${it.value.title}` : it.value.type === 'season' ? `${it.value.parentTitle} · ${seasonTitle(it.value.title)}` : it.value.title));
const facts = computed(() => [it.value.year, it.value.type === 'show' ? t('{n} episodes', { n: it.value.leafCount || 0 }) : it.value.duration ? fmtDur(it.value.duration) : '',
  ...(it.value.genres || []).slice(0, 3)].filter(Boolean).join(' · '));
const at = computed(() => resumeAt(it.value));
const go = (id: number, from: number) => { emit('close'); router.push({ name: 'play', params: { id }, query: { t: String(Math.floor(from)) } }); };
// A show or season: its next episode not watched yet (or the first).
async function playNext() {
  const eps = list<Item>(await get(`/library/metadata/${it.value.id}/allLeaves`).catch(() => null));
  const next = eps.find(e => (e.viewOffset || 0) > 0 && !e.viewCount) || eps.find(e => !e.viewCount) || eps[0];
  if (next) go(next.id, resumeAt(next));
}
function open() { emit('close'); router.push('/item/' + it.value.id); }
</script>

<template>
  <Modal :title="t('At random')" wide @close="emit('close')">
    <div class="pickone" @click.capture="($event.target as Element).closest('a') && emit('close')">
      <PosterCard :item="it" />
      <div>
        <h3>{{ name }}</h3>
        <div v-if="facts" class="dim">{{ facts }}</div>
        <p v-if="it.summary" class="pick-sum">{{ it.summary }}</p>
      </div>
    </div>
    <template #actions>
      <button v-if="isPlayable(it) && !it.missing" class="primary" @click="go(it.id, at)">{{ at ? t('▶ Resume from {time}', { time: fmtTime(at) }) : t('▶ Play') }}</button>
      <button v-else-if="(it.type === 'show' || it.type === 'season') && it.leafCount" class="primary" @click="playNext">{{ t('▶ Play next episode') }}</button>
      <button @click="open">{{ t('More info') }}</button>
      <button @click="emit('again')">{{ t('🎲 Another one') }}</button>
      <button @click="emit('close')">{{ t('Close') }}</button>
    </template>
  </Modal>
</template>
