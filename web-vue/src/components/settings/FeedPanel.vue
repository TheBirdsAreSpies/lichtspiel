<script setup lang="ts">
// Settings → Watchlist feed: your watchlist as RSS. The address holds a private key, so a feed reader needs no
// sign-in; a new address makes the old one stop working.
import { onMounted, ref } from 'vue';
import { api, get } from '../../api';
import { t } from '../../i18n';
import Modal from '../Modal.vue';

const feed = ref<{ enabled: boolean, url?: string } | null>(null);
const error = ref('');
const copied = ref(false);
const renewing = ref(false);
onMounted(async () => { try { feed.value = await get('/api/rss'); } catch (e) { error.value = (e as Error).message; } });
async function post(body: Record<string, boolean>) {
  renewing.value = false;
  try { feed.value = await api('POST', '/api/rss', body); copied.value = false; } catch (e) { error.value = (e as Error).message; }
}
async function copy() {
  if (!feed.value || !feed.value.url) return;
  try { await navigator.clipboard.writeText(feed.value.url); copied.value = true; } catch { /* the field can still be selected by hand */ }
}
</script>

<template>
  <section class="panel">
    <h2>{{ t('Watchlist feed') }}</h2>
    <p class="dim">{{ t('Your watchlist as an RSS feed for a feed reader or an automation. Anyone with the address can read the list, so keep it private.') }}</p>
    <p v-if="error" class="err">{{ error }}</p>
    <template v-if="feed">
      <template v-if="feed.enabled">
        <input class="full" readonly :value="feed.url" :aria-label="t('Watchlist feed')" @focus="($event.target as HTMLInputElement).select()">
        <div class="actions">
          <button class="primary" @click="copy">{{ copied ? t('Link copied') : t('Copy link') }}</button>
          <button @click="renewing = true">{{ t('New address') }}</button>
          <button class="danger" @click="post({ enabled: false })">{{ t('Switch off') }}</button>
        </div>
      </template>
      <div v-else class="actions"><button class="primary" @click="post({ enabled: true })">{{ t('Switch on') }}</button></div>
    </template>
    <Modal v-if="renewing" :title="t('Make a new address? The old one stops working.')" @close="renewing = false">
      <template #actions>
        <button class="primary" @click="post({ regenerate: true })">{{ t('New address') }}</button>
        <button @click="renewing = false">{{ t('Cancel') }}</button>
      </template>
    </Modal>
  </section>
</template>
