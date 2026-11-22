<script setup lang="ts">
// A room's link (/together/<code>): joins it and plays what it plays, from where it is.
import { ref } from 'vue';
import { useRouter } from 'vue-router';
import { t } from '../i18n';
import { joinRoom } from '../together';

const props = defineProps<{ code: string }>();
const router = useRouter();
const error = ref('');
joinRoom(props.code)
  .then(st => router.replace({ path: '/play/' + st.item, query: { t: String(st.position / 1000), together: '1', joined: '1', ...(st.paused ? { paused: '1' } : {}) } }))
  .catch(e => { error.value = (e as Error).message; });
</script>

<template>
  <p v-if="error" class="empty">{{ error }}</p>
  <p v-else class="empty">{{ t('Loading…') }}</p>
</template>
