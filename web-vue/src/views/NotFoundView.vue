<script setup lang="ts">
// An address the app does not know (an old link, a typo): says so, with the way home.
import { t } from '../i18n';
</script>

<template>
  <div class="empty">
    <p>{{ t('Page not found.') }}</p>
    <RouterLink to="/" class="btn primary">{{ t('Home') }}</RouterLink>
  </div>
</template>
