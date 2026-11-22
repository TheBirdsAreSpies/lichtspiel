<script setup lang="ts">
// Signing in: with a password (and the code of two-step sign-in when it is on), by picking a home profile
// (only offered in the local network, with its PIN), or on a new server by making the owner account.
import { computed, onMounted, ref } from 'vue';
import { useRoute, useRouter } from 'vue-router';
import { api, ApiError, get, session, signIn } from '../api';
import { lang, packs, setLang, t } from '../i18n';

interface Profile { id: number, username: string, display_name?: string, has_pin?: boolean }

const route = useRoute();
const router = useRouter();
const setup = computed(() => !!(session.status && session.status.setup_needed));
const username = ref('');
const password = ref('');
const code = ref('');
const needsCode = ref(false);
const pin = ref('');
const profile = ref<Profile | null>(null);
const profiles = ref<Profile[]>([]);
const error = ref('');
const busy = ref(false);

onMounted(async () => {
  if (setup.value) return;
  try { profiles.value = (await get<{ profiles: Profile[] }>('/api/home')).profiles; } catch { /* not in the local network */ }
});

async function done(token: string) {
  await signIn(token);
  if (!session.me) { error.value = t('Please sign in again'); return; }
  const next = typeof route.query.next === 'string' && route.query.next.startsWith('/') ? route.query.next : '/';
  router.replace(next);
}

async function run(f: () => Promise<void>) {
  error.value = ''; busy.value = true;
  try { await f(); } catch (e) {
    if (e instanceof ApiError && e.data && e.data.needs_code) { needsCode.value = true; return; }
    error.value = e instanceof Error ? e.message : String(e);
  } finally { busy.value = false; }
}

const submit = () => run(async () => {
  if (setup.value) {
    const r = await api<{ token: string }>('POST', '/api/setup', { username: username.value, password: password.value });
    return done(r.token);
  }
  const body: Record<string, string> = { username: username.value, password: password.value, device: 'web' };
  if (needsCode.value) body.code = code.value;
  const r = await api<{ token: string }>('POST', '/api/login', body);
  await done(r.token);
});

const pick = (p: Profile) => {
  pin.value = '';
  if (p.has_pin) { profile.value = p; return; }
  profileSignIn(p);
};
const profileSignIn = (p: Profile) => run(async () => {
  const r = await api<{ token: string }>('POST', `/api/home/${p.id}/login`, { pin: pin.value, device: 'web' });
  await done(r.token);
});
const shown = (p: Profile) => p.display_name || p.username;
</script>

<template>
  <div class="auth">
    <form v-if="profile" class="auth-box" @submit.prevent="profileSignIn(profile)">
      <h1>{{ shown(profile) }}</h1>
      <label for="pin">{{ t('PIN') }}</label>
      <input id="pin" v-model="pin" type="password" inputmode="numeric" autocomplete="off" autofocus>
      <button class="primary wide" type="submit" :disabled="busy">{{ t('Sign in') }}</button>
      <button class="wide" type="button" @click="profile = null">{{ t('Back') }}</button>
      <p v-if="error" class="err">{{ error }}</p>
    </form>

    <form v-else class="auth-box" @submit.prevent="submit">
      <h1>{{ setup ? t('Welcome') : needsCode ? t('Two-step sign-in') : t('Sign in') }}</h1>
      <p v-if="setup" class="dim">{{ t('Create the owner account for this server. You can invite other people afterwards.') }}</p>
      <p v-else-if="session.status" class="dim">{{ session.status.name }}</p>

      <template v-if="needsCode">
        <p class="dim">{{ t('Enter the 6-digit code from your authenticator app.') }}</p>
        <label for="code">{{ t('Code') }}</label>
        <input id="code" v-model="code" autocomplete="one-time-code" inputmode="numeric" autofocus>
      </template>
      <template v-else>
        <div v-if="profiles.length && !setup" class="profiles">
          <button v-for="p in profiles" :key="p.id" type="button" class="profile" :title="p.username" @click="pick(p)">
            <span class="av big">{{ (shown(p)[0] || '?').toUpperCase() }}</span>
            <span>{{ shown(p) }}<span v-if="p.has_pin" class="dim"> 🔒</span></span>
          </button>
        </div>
        <div v-if="profiles.length && !setup" class="or dim">{{ t('or sign in with a password') }}</div>
        <label for="user">{{ t('User name') }}</label>
        <input id="user" v-model="username" autocomplete="username" autofocus>
        <label for="pw">{{ t('Password') }}</label>
        <input id="pw" v-model="password" type="password" :autocomplete="setup ? 'new-password' : 'current-password'">
      </template>

      <button class="primary wide" type="submit" :disabled="busy">{{ setup ? t('Create account') : t('Sign in') }}</button>
      <p v-if="error" class="err">{{ error }}</p>
      <div class="auth-foot">
        <a href="/" class="dim">{{ t('Classic web app') }}</a>
        <select :value="lang" :aria-label="t('Language')" @change="setLang(($event.target as HTMLSelectElement).value)">
          <option v-for="(p, c) in packs" :key="c" :value="c">{{ p.name }}</option>
        </select>
      </div>
    </form>
  </div>
</template>
