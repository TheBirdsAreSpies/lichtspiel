<script setup lang="ts">
// Asks for one line of text (a name). Emits done with the text, or with null when cancelled. With allowEmpty
// an empty answer counts too ("": take a PIN away).
import { ref } from 'vue';
import { t } from '../i18n';
import Modal from './Modal.vue';

const props = defineProps<{ title: string, value?: string, ok?: string, password?: boolean, hint?: string, placeholder?: string, allowEmpty?: boolean }>();
const emit = defineEmits<{ done: [string | null] }>();
const text = ref(props.value || '');
// (A password is taken as typed, spaces too.)
const submit = () => { if (text.value.trim() || props.allowEmpty) emit('done', props.password ? text.value : text.value.trim()); };
</script>

<template>
  <Modal :title="title" @close="emit('done', null)">
    <form @submit.prevent="submit">
      <input v-model="text" class="full" :aria-label="title" :type="password ? 'password' : 'text'" :placeholder="placeholder"
        :autocomplete="password ? 'current-password' : 'off'">
    </form>
    <p v-if="hint" class="dim">{{ hint }}</p>
    <template #actions>
      <button class="primary" :disabled="!text.trim() && !allowEmpty" @click="submit">{{ ok || t('Save') }}</button>
      <button @click="emit('done', null)">{{ t('Cancel') }}</button>
    </template>
  </Modal>
</template>
