<script setup lang="ts">
// Report a problem with a title (does not play, picture, sound, subtitles, details …): the owner sees it
// under Reported problems.
import { ref } from 'vue';
import { api, type Item } from '../api';
import { t } from '../i18n';
import { PROBLEMS } from '../reports';
import { toast } from '../toast';
import Modal from './Modal.vue';

const props = defineProps<{ item: Item }>();
const emit = defineEmits<{ close: [] }>();
const kind = ref('playback');
const note = ref('');
const busy = ref(false);
async function send() {
  busy.value = true;
  try { await api('POST', `/library/metadata/${props.item.id}/report`, { kind: kind.value, note: note.value }); toast(t('Thanks — the problem was reported')); emit('close'); }
  catch (e) { toast((e as Error).message, 5000); } finally { busy.value = false; }
}
</script>

<template>
  <Modal :title="t('Report a problem')" @close="emit('close')">
    <p class="dim">{{ t('The owner of this server sees your report.') }}</p>
    <div class="form">
      <label for="rp-kind">{{ t('Problem') }}</label>
      <select id="rp-kind" v-model="kind"><option v-for="[v, l] in PROBLEMS" :key="v" :value="v">{{ t(l) }}</option></select>
      <label for="rp-note">{{ t('Details') }}</label>
      <textarea id="rp-note" v-model="note" rows="4" :placeholder="t('What happens? (optional)')"></textarea>
    </div>
    <template #actions>
      <button class="primary" :disabled="busy" @click="send">{{ t('Send') }}</button>
      <button @click="emit('close')">{{ t('Cancel') }}</button>
    </template>
  </Modal>
</template>
