<script setup lang="ts">
// "Queue (n)" at the top while something is queued (queue.ts): opens the list to reorder, take off, play or clear.
import { ref } from 'vue';
import { useRouter } from 'vue-router';
import { picture } from '../api';
import { fmtDur } from '../format';
import { t } from '../i18n';
import { queueTitle, saveQueue, upQueue } from '../queue';
import Modal from './Modal.vue';

const router = useRouter();
const open = ref(false);
function up(i: number) { upQueue.list.splice(i - 1, 0, upQueue.list.splice(i, 1)[0]); saveQueue(); }
function drop(i: number) { upQueue.list.splice(i, 1); saveQueue(); if (!upQueue.list.length) open.value = false; }
function clear() { upQueue.list = []; saveQueue(); open.value = false; }
function playIt() {
  open.value = false;
  const first = upQueue.list.shift(); saveQueue();
  if (first) router.push({ name: 'play', params: { id: first.id }, query: { t: '0' } });
}
</script>

<template>
  <button v-if="upQueue.list.length" class="small qchip" @click="open = true">{{ t('Queue ({n})', { n: upQueue.list.length }) }}</button>
  <Modal v-if="open" :title="t('Queue')" wide @close="open = false">
    <p class="dim">{{ t('Plays one after the other, after whatever is playing now.') }}</p>
    <div class="list qlist">
      <div v-for="(x, i) in upQueue.list" :key="i + ':' + x.id" class="li">
        <img v-if="x.thumb" :src="picture(x.thumb, 160)" loading="lazy" alt="">
        <div class="grow"><b>{{ queueTitle(x) }}</b><div class="dim">{{ fmtDur(x.duration) }}</div></div>
        <button v-if="i" class="small" :title="t('Move up')" :aria-label="t('Move up')" @click="up(i)">↑</button>
        <button class="small" :title="t('Remove')" :aria-label="t('Remove')" @click="drop(i)">✕</button>
      </div>
    </div>
    <template #actions>
      <button class="primary" @click="playIt">{{ t('▶ Play the queue') }}</button>
      <button class="danger" @click="clear">{{ t('Clear the queue') }}</button>
      <button @click="open = false">{{ t('Close') }}</button>
    </template>
  </Modal>
</template>
