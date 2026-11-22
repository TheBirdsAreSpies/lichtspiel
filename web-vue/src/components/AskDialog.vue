<script setup lang="ts">
// Asks for one line of text (a name). Emits done with the text, or with null when cancelled.
import { ref } from 'vue';
import { t } from '../i18n';
import Modal from './Modal.vue';

const props = defineProps<{ title: string, value?: string, ok?: string }>();
const emit = defineEmits<{ done: [string | null] }>();
const text = ref(props.value || '');
const submit = () => { if (text.value.trim()) emit('done', text.value.trim()); };
</script>

<template>
  <Modal :title="title" @close="emit('done', null)">
    <form @submit.prevent="submit"><input v-model="text" class="full" :aria-label="title"></form>
    <template #actions>
      <button class="primary" :disabled="!text.trim()" @click="submit">{{ ok || t('Save') }}</button>
      <button @click="emit('done', null)">{{ t('Cancel') }}</button>
    </template>
  </Modal>
</template>
