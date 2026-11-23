<script setup lang="ts">
// Picks a folder on the server (GET /api/browse): starts at the drives, a click opens a folder, "Up" goes back.
// Emits done with the folder, or with null when cancelled.
import { onMounted, ref } from 'vue';
import { get } from '../api';
import { t } from '../i18n';
import { toast } from '../toast';
import Modal from './Modal.vue';

const emit = defineEmits<{ done: [string | null] }>();
interface Browse { path: string, parent?: string | null, dirs: { name: string, path: string }[], videos_here?: number }
const cur = ref<Browse | null>(null);

async function open(p: string) {
  try { cur.value = await get<Browse>('/api/browse?path=' + encodeURIComponent(p)); } catch (e) { toast((e as Error).message); }
}
onMounted(() => open(''));
</script>

<template>
  <Modal :title="t('Pick a folder')" @close="emit('done', null)">
    <p class="dim path">{{ cur ? (cur.path || t('Drives')) + (cur.videos_here ? ' — ' + t('{n} videos here', { n: cur.videos_here }) : '') : '' }}</p>
    <div class="picker">
      <button v-if="cur && cur.parent !== null && cur.parent !== undefined" class="row" @click="open(cur.parent)">⬆ {{ t('Up') }}</button>
      <button v-for="d in cur?.dirs || []" :key="d.path" class="row" @click="open(d.path)">📁 {{ d.name }}</button>
    </div>
    <template #actions>
      <button class="primary" :disabled="!cur || !cur.path" @click="cur && emit('done', cur.path)">{{ t('Use this folder') }}</button>
      <button @click="emit('done', null)">{{ t('Cancel') }}</button>
    </template>
  </Modal>
</template>

<style scoped>
.path { word-break: break-all; margin-bottom: 8px; }
.picker { max-height: 50vh; overflow: auto; display: flex; flex-direction: column; }
.row { background: none; border: 0; border-bottom: 1px solid var(--line); border-radius: 0; text-align: left; padding: 9px 6px; justify-content: flex-start; }
.row:hover { background: var(--panel2); }
</style>
