<script setup lang="ts">
// Settings → Devices: the browsers, apps and scripts signed in to your account (GET /api/tokens), the one
// used last first; any but this one can be signed out.
import { onMounted, ref } from 'vue';
import { api, get } from '../../api';
import { lang, t } from '../../i18n';
import Modal from '../Modal.vue';

interface Token { id: string, device?: string, last_device?: string, last_used?: number, last_ip?: string, created_at: number, current?: boolean }
const tokens = ref<Token[] | null>(null);
const error = ref('');
const asking = ref<Token | null>(null);

async function load() {
  try {
    const r = await get<{ tokens: Token[] }>('/api/tokens');
    tokens.value = r.tokens.sort((a, b) => (b.last_used || b.created_at) - (a.last_used || a.created_at));
  } catch (e) { error.value = (e as Error).message; }
}
onMounted(load);
async function signOut(k: Token) {
  asking.value = null;
  try { await api('DELETE', '/api/tokens/' + encodeURIComponent(k.id)); await load(); } catch (e) { error.value = (e as Error).message; }
}
const when = (s?: number) => (s ? new Date(s * 1000).toLocaleString(lang.value, { dateStyle: 'medium', timeStyle: 'short' }) : '–');
</script>

<template>
  <section class="panel">
    <h2>{{ t('Active devices') }}</h2>
    <p class="dim">{{ t('Browsers, apps and scripts that are signed in to your account.') }}</p>
    <p v-if="error" class="err">{{ error }}</p>
    <div v-if="tokens === null" class="skel line"></div>
    <p v-else-if="!tokens.length" class="dim">{{ t('No devices.') }}</p>
    <div v-else class="devices">
      <div v-for="k in tokens" :key="k.id" class="device">
        <div class="grow">
          <b>{{ k.last_device || k.device || t('Web app') }}</b>
          <span v-if="k.current" class="tag here">{{ t('this device') }}</span>
          <div v-if="k.device && k.last_device && k.device !== k.last_device" class="dim">{{ k.device }}</div>
          <div class="dim small">
            {{ t('Last used') }}: {{ when(k.last_used) }}<template v-if="k.last_ip"> · {{ k.last_ip }}</template> · {{ t('Signed in') }}: {{ when(k.created_at) }}
          </div>
        </div>
        <button v-if="!k.current" class="danger" @click="asking = k">{{ t('Sign out') }}</button>
      </div>
    </div>
    <Modal v-if="asking" :title="t('Sign this device out? It has to sign in again.')" @close="asking = null">
      <p><b>{{ asking.last_device || asking.device || t('Web app') }}</b></p>
      <template #actions>
        <button class="danger" @click="signOut(asking)">{{ t('Sign out') }}</button>
        <button @click="asking = null">{{ t('Cancel') }}</button>
      </template>
    </Modal>
  </section>
</template>
