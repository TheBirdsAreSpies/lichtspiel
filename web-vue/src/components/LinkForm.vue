<script setup lang="ts">
// Sign in a TV or app: it shows a short code ("Sign in with a code", in a browser "Sign in with your
// phone"), it is typed (or comes from the QR code's address) here, and that device is signed in as you.
import { ref } from 'vue';
import { api, session } from '../api';
import { t } from '../i18n';

const props = defineProps<{ code?: string }>();
const code = ref(props.code || '');
const msg = ref('');
const ok = ref(false);
async function go() {
  msg.value = ''; ok.value = false;
  try {
    const r = await api<{ device?: string }>('POST', '/api/link/confirm', { code: code.value });
    code.value = ''; ok.value = true;
    msg.value = t('{device} is signed in as {name}.', { device: r.device || t('The device'), name: session.me?.username || '' });
  } catch (e) { msg.value = (e as Error).message; }
}
</script>

<template>
  <div>
    <p class="dim">{{ t('Open the app on your TV or phone and choose "Sign in with a code" (in a browser: "Sign in with your phone"). Type the code it shows here.') }}</p>
    <input v-model="code" class="code-input" maxlength="9" placeholder="ABCD" autocomplete="off" autocapitalize="characters" spellcheck="false" :aria-label="t('Code')" @keydown.enter.prevent="go">
    <div class="actions"><button class="primary" :disabled="!code.trim()" @click="go">{{ t('Sign in device') }}</button><slot /></div>
    <p v-if="msg" :class="ok ? 'ok' : 'err'">{{ msg }}</p>
  </div>
</template>
