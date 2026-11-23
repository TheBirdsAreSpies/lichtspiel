<script setup lang="ts">
// Settings → Account: name and e-mail address, the language, signing in automatically, the password and
// two-step sign-in (a code from an authenticator app).
import { ref } from 'vue';
import { api, get, pref, savePref, session, type Me } from '../../api';
import { applyLook } from '../../look';
import { stopTheme } from '../../theme';
import { toast } from '../../toast';
import { lang, packs, setLang, t } from '../../i18n';

const me = session.me!;
const fullName = ref(me.full_name || '');
const email = ref(me.email || '');
const msg = ref<Record<string, string>>({});
const err = ref<Record<string, string>>({});
/** A note under a part of the page: done, or what went wrong. */
function say(k: string, text: string, bad = false) {
  msg.value = { ...msg.value, [k]: bad ? '' : text };
  err.value = { ...err.value, [k]: bad ? text : '' };
}

async function saveProfile() {
  try {
    const x = await api<{ full_name?: string, email?: string }>('POST', '/api/me/profile', { full_name: fullName.value, email: email.value });
    me.full_name = x.full_name; me.email = x.email; me.display_name = x.full_name || me.username;
    say('profile', t('Saved'));
  } catch (e) { say('profile', (e as Error).message, true); }
}

function chooseLang(code: string) { setLang(code); savePref('lang', code).catch(() => {}); }
const keep = ref((() => { try { return localStorage.getItem('autoLogin') !== '0'; } catch { return true; } })());
function setKeep(v: boolean) {
  keep.value = v;
  try {
    localStorage.setItem('autoLogin', v ? '1' : '0');
    // (Kept per browser, as in the classic app.) The sign-in moves to where it is kept now: for good, or until the browser is closed.
    if (v) { localStorage.setItem('token', session.token); sessionStorage.removeItem('token'); }
    else { sessionStorage.setItem('token', session.token); localStorage.removeItem('token'); }
  } catch { /* storage blocked */ }
}

// ---- how the app looks and behaves, kept with the account (the classic app's settings of the same name)
const ROWS: { key: string, def: string, label: string, opts: [string, string][], help: string }[] = [
  { key: 'theme', def: 'system', label: 'Appearance', opts: [['system', 'System'], ['light', 'Light'], ['dark', 'Dark'], ['oled', 'OLED (pure black)']], help: 'Follows the device’s setting. Saved with your account.' },
  { key: 'accent', def: 'rose', label: 'Accent color', opts: [['rose', 'Rose'], ['amber', 'Amber'], ['blue', 'Blue'], ['green', 'Green'], ['red', 'Red'], ['violet', 'Violet']], help: 'Saved with your account.' },
  { key: 'timeFormat', def: 'auto', label: 'Time format', opts: [['auto', 'Automatic'], ['24', '24-hour'], ['12', '12-hour']], help: 'Saved with your account.' },
  { key: 'levelVolume', def: '0', label: 'Level the volume', opts: [['0', 'No'], ['1', 'Yes']], help: 'Quiet and loud videos are brought to a similar loudness. Works for videos the server has measured (Settings → Scheduled tasks).' },
  { key: 'themeMusic', def: '1', label: 'Play theme music', opts: [['0', 'No'], ['1', 'Yes']], help: 'Plays theme music quietly on a movie’s or show’s page: a theme.mp3 from its folder, or for shows the music cut from the intro. Browsers may need one click on the page first.' },
  { key: 'themeLoop', def: '1', label: 'Theme music repeats', opts: [['1', 'Over and over'], ['0', 'Once']], help: 'Over and over: until you leave the page or start playing. Once: it stops at its end.' },
  { key: 'rememberTab', def: '0', label: 'Remember the last page', opts: [['0', 'No'], ['1', 'Yes']], help: 'When the app is opened without an address, it continues where you left off.' },
];
const vals = ref<Record<string, string>>(Object.fromEntries(ROWS.map(x => [x.key, pref(x.key, x.def)])));
async function setRow(key: string, value: string) {
  vals.value = { ...vals.value, [key]: value };
  const p = savePref(key, value);
  if (key === 'theme' || key === 'accent') applyLook();
  if (key === 'themeMusic' && value !== '1') stopTheme();
  try { await p; toast(t('Saved')); } catch (e) { toast((e as Error).message); }
}

// ---- password
const oldPw = ref(''), newPw = ref(''), newPw2 = ref('');
async function changePassword() {
  if (newPw.value !== newPw2.value) return say('pw', t('The two new passwords are not the same.'), true);
  try {
    await api('POST', '/api/me/password', { old: oldPw.value, new: newPw.value });
    oldPw.value = newPw.value = newPw2.value = '';
    say('pw', t('Password changed'));
  } catch (e) { say('pw', (e as Error).message, true); }
}

// ---- two-step sign-in
const two = ref(!!me.two_factor);
const setup = ref<{ secret: string, uri: string, qr: string } | null>(null);
const code = ref('');
const offPw = ref(''), offCode = ref('');
const turningOff = ref(false);
async function refreshTwo() { const fresh = await get<Me>('/api/me'); two.value = !!fresh.two_factor; me.two_factor = fresh.two_factor; }
async function startSetup() {
  try { setup.value = await api('POST', '/api/me/2fa/setup'); code.value = ''; say('two', ''); } catch (e) { say('two', (e as Error).message, true); }
}
async function enable() {
  try { await api('POST', '/api/me/2fa/enable', { code: code.value.trim() }); setup.value = null; await refreshTwo(); say('two', t('Two-step sign-in is on.')); }
  catch (e) { say('two', (e as Error).message, true); }
}
async function disable() {
  try {
    await api('POST', '/api/me/2fa/disable', { password: offPw.value, code: offCode.value.trim() });
    offPw.value = offCode.value = ''; turningOff.value = false;
    await refreshTwo(); say('two', t('Saved'));
  } catch (e) { say('two', (e as Error).message, true); }
}
</script>

<template>
  <section class="panel">
    <h2>{{ t('Your account') }}</h2>
    <div class="form">
      <label>{{ t('User name') }}</label><div class="val"><b>{{ me.username }}</b></div>
      <label for="s-name">{{ t('Full name') }}</label><input id="s-name" v-model="fullName" autocomplete="name">
      <label for="s-mail">{{ t('E-mail address') }}</label>
      <div><input id="s-mail" v-model="email" type="email" placeholder="name@example.com" autocomplete="email"><p class="dim hint">{{ t('For a new password when you forgot yours.') }}</p></div>
    </div>
    <div class="actions"><button class="primary" @click="saveProfile">{{ t('Save') }}</button><span v-if="msg.profile" class="ok-msg">{{ msg.profile }}</span><span v-if="err.profile" class="err">{{ err.profile }}</span></div>
  </section>

  <section class="panel">
    <h2>{{ t('This app') }}</h2>
    <div class="form">
      <label for="s-lang">{{ t('Language of this app') }}</label>
      <div>
        <select id="s-lang" :value="lang" @change="chooseLang(($event.target as HTMLSelectElement).value)">
          <option v-for="(p, c) in packs" :key="c" :value="c">{{ p.name }}</option>
        </select>
        <p class="dim hint">{{ t('Saved with your account.') }}</p>
      </div>
      <label for="s-keep">{{ t('Sign in automatically') }}</label>
      <div>
        <select id="s-keep" :value="keep ? '1' : '0'" @change="setKeep(($event.target as HTMLSelectElement).value === '1')">
          <option value="1">{{ t('Yes') }}</option><option value="0">{{ t('No') }}</option>
        </select>
        <p class="dim hint">{{ t('No: you are signed out when the browser is closed.') }}</p>
      </div>
      <template v-for="row in ROWS" :key="row.key">
        <label :for="'s-' + row.key">{{ t(row.label) }}</label>
        <div>
          <select :id="'s-' + row.key" :value="vals[row.key]" @change="setRow(row.key, ($event.target as HTMLSelectElement).value)">
            <option v-for="[v, l] in row.opts" :key="v" :value="v">{{ t(l) }}</option>
          </select>
          <p class="dim hint">{{ t(row.help) }}</p>
        </div>
      </template>
    </div>
  </section>

  <section class="panel">
    <h2>{{ t('Password') }}</h2>
    <form class="form" @submit.prevent="changePassword">
      <input type="text" :value="me.username" autocomplete="username" hidden>
      <label for="s-old">{{ t('Current password') }}</label><input id="s-old" v-model="oldPw" type="password" autocomplete="current-password">
      <label for="s-new">{{ t('New password') }}</label>
      <div><input id="s-new" v-model="newPw" type="password" autocomplete="new-password" minlength="6"><p class="dim hint">{{ t('At least 6 characters.') }}</p></div>
      <label for="s-new2">{{ t('Repeat the new password') }}</label><input id="s-new2" v-model="newPw2" type="password" autocomplete="new-password">
      <span></span>
      <div class="actions"><button class="primary" :disabled="!oldPw || newPw.length < 6 || !newPw2">{{ t('Change my password') }}</button><span v-if="msg.pw" class="ok-msg">{{ msg.pw }}</span><span v-if="err.pw" class="err">{{ err.pw }}</span></div>
    </form>
  </section>

  <section class="panel">
    <h2>{{ t('Two-step sign-in') }}</h2>
    <template v-if="two">
      <p class="ok-msg">{{ t('Two-step sign-in is on.') }}</p>
      <button v-if="!turningOff" class="danger" @click="turningOff = true">{{ t('Switch off') }}</button>
      <form v-else class="form" @submit.prevent="disable">
        <label for="s-offpw">{{ t('Your password') }}</label><input id="s-offpw" v-model="offPw" type="password" autocomplete="current-password">
        <label for="s-offcode">{{ t('Code from your authenticator app') }}</label><input id="s-offcode" v-model="offCode" inputmode="numeric" autocomplete="one-time-code" placeholder="123456">
        <span></span>
        <div class="actions"><button class="danger" :disabled="!offPw || !offCode">{{ t('Switch off') }}</button><button type="button" @click="turningOff = false">{{ t('Cancel') }}</button></div>
      </form>
    </template>
    <template v-else-if="setup">
      <p>{{ t('Scan this picture with your authenticator app:') }}</p>
      <!-- (An SVG made by the server from the link.) -->
      <div class="qr" v-html="setup.qr"></div>
      <p class="dim">{{ t('Or type the key in, or open the link on your phone:') }}</p>
      <p><code class="secret">{{ setup.secret }}</code></p>
      <p><a :href="setup.uri" class="link">{{ t('Open in authenticator app') }}</a></p>
      <form class="actions" @submit.prevent="enable">
        <label for="s-code">{{ t('Then type the 6-digit code it shows:') }}</label>
        <input id="s-code" v-model="code" class="code" inputmode="numeric" autocomplete="one-time-code" placeholder="123456">
        <button class="primary" :disabled="!code.trim()">{{ t('Switch on') }}</button>
        <button type="button" @click="setup = null">{{ t('Cancel') }}</button>
      </form>
    </template>
    <template v-else>
      <p class="dim">{{ t('Ask for a code from an authenticator app (Google Authenticator, Aegis, 1Password …) when you sign in with a password.') }}</p>
      <button @click="startSetup">{{ t('Set up') }}</button>
    </template>
    <p v-if="msg.two && !two" class="ok-msg">{{ msg.two }}</p>
    <p v-if="err.two" class="err">{{ err.two }}</p>
  </section>
</template>
