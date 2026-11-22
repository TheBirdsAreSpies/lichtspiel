<script setup lang="ts">
// #/link/CODE: the address in a device's QR code; confirming signs that device in as you.
import { t } from '../i18n';
import LinkForm from '../components/LinkForm.vue';

defineProps<{ code?: string }>();
</script>

<template>
  <div class="narrow">
    <h1>{{ t('Sign in a device') }}</h1>
    <section class="panel"><LinkForm :code="code" /></section>
  </div>
</template>
