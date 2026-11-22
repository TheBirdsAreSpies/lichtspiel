<script setup lang="ts">
// The other ways in, next to the sign-in with a password (LoginView):
// - code: "Sign in with your phone". This page shows a code and a QR code; a phone that is signed in opens
//   #/link/CODE and confirms, and this page asks every few seconds until it may sign in.
// - forgot: a link to a new password by e-mail (when the server can send mail).
// - reset: the link from that mail, to pick the new password.
// - invite: an invite link, to make an account.
import { computed, onBeforeUnmount, onMounted, reactive, ref } from 'vue';
import { useRoute, useRouter } from 'vue-router';
import { api, ApiError, forgetToken, session, signIn } from '../api';
import { lang, t } from '../i18n';

const route = useRoute();
const router = useRouter();
const mode = computed(() => String(route.name));
const code = computed(() => String(route.params.code || ''));
const error = ref('');
const busy = ref(false);
const done = ref(''); // what happened, when the page has nothing more to do
const v = reactive({ name: '', username: '', email: '', password: '', again: '' });
const info = ref<{ server?: string, username?: string, invited_by?: string, note?: string, email?: string } | null>(null);

// ---- code
const shown = ref('…');
const qr = ref('');
let req: { id: string, code: string, qr?: string } | null = null;
let alive = true;
// "Chrome (Android)", "Silk (Fire TV)": the name the phone sees when confirming.
function deviceName() {
  const ua = navigator.userAgent;
  const where = /AFT|Silk/.test(ua) ? 'Fire TV' : /SMART-TV|Tizen|Web0S|webOS|BRAVIA|Android TV|GoogleTV/i.test(ua) ? 'TV'
    : /Android/.test(ua) ? 'Android' : /iPhone|iPad/.test(ua) ? 'iOS' : /Windows/.test(ua) ? 'Windows' : /Mac OS/.test(ua) ? 'Mac' : /Linux/.test(ua) ? 'Linux' : '';
  const app = /Silk/.test(ua) ? 'Silk' : /Firefox/.test(ua) ? 'Firefox' : /Edg\//.test(ua) ? 'Edge' : /Chrome/.test(ua) ? 'Chrome' : /Safari/.test(ua) ? 'Safari' : t('Browser');
  return where ? `${app} (${where})` : app;
}
const linkAddress = location.origin + location.pathname;
async function fresh() {
  try {
    req = await api('POST', '/api/link', { device: deviceName(), link: linkAddress });
    shown.value = req!.code; qr.value = req!.qr || ''; error.value = '';
  } catch (e) { req = null; error.value = (e as Error).message; }
}
async function waitForPhone() {
  await fresh();
  while (alive) {
    await new Promise(r => setTimeout(r, 3000));
    if (!alive) return;
    if (!req) { await fresh(); continue; }
    try {
      const r = await api<{ status: string, token?: string }>('GET', '/api/link/' + req.id);
      if (r.status === 'ok' && r.token && alive) { await signIn(r.token); if (session.me) router.replace('/'); return; }
    } catch (e) { if (e instanceof ApiError && e.status === 410) await fresh(); /* expired: a new code; otherwise just ask again */ }
  }
}

onMounted(async () => {
  if (mode.value === 'code') { waitForPhone(); return; }
  try {
    if (mode.value === 'reset') info.value = await api('GET', '/api/password/reset/' + encodeURIComponent(code.value));
    if (mode.value === 'invite') { info.value = await api('GET', '/api/invites/' + encodeURIComponent(code.value)); v.email = info.value?.email || ''; }
  } catch (e) { error.value = (e as Error).message; info.value = null; done.value = 'failed'; }
});
onBeforeUnmount(() => { alive = false; });

async function run(f: () => Promise<void>) {
  error.value = ''; busy.value = true;
  try { await f(); } catch (e) { error.value = (e as Error).message; } finally { busy.value = false; }
}
const forgot = () => run(async () => {
  await api('POST', '/api/password/forgot', { name: v.name, lang: lang.value });
  done.value = 'sent';
});
const reset = () => run(async () => {
  if (v.password !== v.again) throw new Error(t('The two passwords are not the same'));
  await api('POST', '/api/password/reset/' + encodeURIComponent(code.value), { password: v.password });
  // Signed in here too: that sign-in has just ended.
  if (session.me) forgetToken();
  done.value = 'reset';
});
const join = () => run(async () => {
  const r = await api<{ token: string }>('POST', `/api/invites/${encodeURIComponent(code.value)}/accept`, { username: v.username, email: v.email, password: v.password });
  await signIn(r.token);
  router.replace('/');
});
</script>

<template>
  <div class="auth">
    <div v-if="mode === 'code'" class="auth-box center">
      <h1>{{ t('Sign in with your phone') }}</h1>
      <p class="dim">{{ t('Scan the picture with a phone that is signed in already, and confirm there.') }}</p>
      <!-- (an SVG the server makes from the address) -->
      <div v-if="qr" class="qr" v-html="qr"></div>
      <p class="dim">{{ t('Or open {address} there and type this code:', { address: linkAddress + '#/link' }) }}</p>
      <div class="big-code">{{ shown }}</div>
      <p v-if="error" class="err">{{ error }}</p>
      <RouterLink to="/login" class="btn wide">{{ t('Sign in with a password instead') }}</RouterLink>
    </div>

    <div v-else-if="mode === 'forgot' && done === 'sent'" class="auth-box">
      <h1>{{ t('Look in your mailbox') }}</h1>
      <p>{{ t('If the account has an e-mail address, the link is on its way. It works for one hour.') }}</p>
      <p class="dim">{{ t('Nothing came? Look in the spam folder, or ask an admin to set a new password for you.') }}</p>
      <RouterLink to="/login">{{ t('Back to sign-in') }}</RouterLink>
    </div>
    <form v-else-if="mode === 'forgot'" class="auth-box" @submit.prevent="forgot">
      <h1>{{ t('Forgot your password?') }}</h1>
      <p class="dim">{{ t('Enter your user name or e-mail address. If your account has an e-mail address, a link to set a new password is sent to it.') }}</p>
      <label for="a-name">{{ t('User name or e-mail address') }}</label>
      <input id="a-name" v-model="v.name" autocomplete="username" autofocus>
      <button class="primary wide" type="submit" :disabled="busy">{{ t('Send link') }}</button>
      <p v-if="error" class="err">{{ error }}</p>
      <RouterLink to="/login">{{ t('Back to sign-in') }}</RouterLink>
    </form>

    <div v-else-if="mode === 'reset' && done" class="auth-box">
      <h1>{{ t('New password') }}</h1>
      <p v-if="done === 'reset'">{{ t('Password saved. Sign in with the new one.') }}</p>
      <p v-else class="err">{{ error }}</p>
      <RouterLink v-if="done === 'reset'" to="/login" class="btn primary wide">{{ t('Sign in') }}</RouterLink>
      <RouterLink v-else to="/forgot">{{ t('Ask for a new link') }}</RouterLink>
    </div>
    <form v-else-if="mode === 'reset'" class="auth-box" @submit.prevent="reset">
      <h1>{{ t('New password') }}</h1>
      <p v-if="info" class="dim">{{ t('For {name} on {server}. Every device signed in with this account is signed out.', { name: info.username || '', server: info.server || '' }) }}</p>
      <label for="a-pw">{{ t('New password (at least 6 characters)') }}</label>
      <input id="a-pw" v-model="v.password" type="password" autocomplete="new-password" autofocus>
      <label for="a-pw2">{{ t('The same password again') }}</label>
      <input id="a-pw2" v-model="v.again" type="password" autocomplete="new-password">
      <button class="primary wide" type="submit" :disabled="busy || !info">{{ t('Save password') }}</button>
      <p v-if="error" class="err">{{ error }}</p>
    </form>

    <div v-else-if="mode === 'invite' && done" class="auth-box">
      <h1>{{ t('Invite') }}</h1>
      <p class="err">{{ error }}</p>
      <RouterLink to="/login">{{ t('Sign in instead') }}</RouterLink>
    </div>
    <form v-else-if="mode === 'invite'" class="auth-box" @submit.prevent="join">
      <h1>{{ t('Join {server}', { server: info?.server || '' }) }}</h1>
      <p v-if="info" class="dim">{{ t('{who} invited you{note}. Pick a user name and password.', { who: info.invited_by || t('Someone'), note: info.note ? ': ' + info.note : '' }) }}</p>
      <label for="a-user">{{ t('User name') }}</label>
      <input id="a-user" v-model="v.username" autocomplete="username" autofocus>
      <label for="a-mail">{{ t('Email (optional)') }}</label>
      <input id="a-mail" v-model="v.email" type="email" autocomplete="email">
      <label for="a-pw3">{{ t('Password (at least 6 characters)') }}</label>
      <input id="a-pw3" v-model="v.password" type="password" autocomplete="new-password">
      <button class="primary wide" type="submit" :disabled="busy || !info">{{ t('Join') }}</button>
      <p v-if="error" class="err">{{ error }}</p>
    </form>
  </div>
</template>
