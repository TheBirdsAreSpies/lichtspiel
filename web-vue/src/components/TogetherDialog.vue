<script setup lang="ts">
// "Watch together" on an item: opens a room for it (a show or season: its first unwatched episode), lets
// you ask people in or send the link, and starts watching.
import { ref } from 'vue';
import { useRouter } from 'vue-router';
import { api, get, list, type Item } from '../api';
import { t } from '../i18n';
import { toast } from '../toast';
import Modal from './Modal.vue';
import TogetherPeople from './TogetherPeople.vue';

const props = defineProps<{ item: Item }>();
const emit = defineEmits<{ close: [] }>();
const router = useRouter();
const code = ref('');
const error = ref('');
const url = ref('');

(async () => {
  try {
    const i = props.item;
    const leaves = ['show', 'season'].includes(i.type) ? list<Item>(await get(`/library/metadata/${i.id}/allLeaves`)) : [i];
    const target = leaves.find(x => !x.viewCount) || leaves[0];
    if (!target) { error.value = t('Nothing to play'); return; }
    const r = await api<{ code: string }>('POST', '/api/together', { item: target.id, position: target.viewOffset || 0 });
    code.value = r.code;
    url.value = location.origin + location.pathname + '#/together/' + r.code;
  } catch (e) { error.value = (e as Error).message; }
})();
function copy() { navigator.clipboard?.writeText(url.value).then(() => toast(t('Link copied'))).catch(() => {}); }
function start() { emit('close'); router.push('/together/' + code.value); }
</script>

<template>
  <Modal :title="t('Watch together')" @close="emit('close')">
    <p v-if="error" class="err">{{ error }}</p>
    <p v-else-if="!code" class="dim">{{ t('Loading…') }}</p>
    <template v-else>
      <p class="dim">{{ t('Whoever plays, pauses or jumps moves everyone.') }}</p>
      <h3>{{ t('People here') }}</h3>
      <TogetherPeople :code="code" />
      <h3>{{ t('Or send the link') }}</h3>
      <p class="dim">{{ t('For someone with an account here that may see this library.') }}</p>
      <input class="full" :value="url" readonly @click="($event.target as HTMLInputElement).select()">
    </template>
    <template #actions>
      <button class="primary" :disabled="!code" @click="start">{{ t('▶ Start watching') }}</button>
      <button :disabled="!code" @click="copy">{{ t('Copy link') }}</button>
      <button @click="emit('close')">{{ t('Close') }}</button>
    </template>
  </Modal>
</template>
