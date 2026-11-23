<script setup lang="ts">
// The frame around every page: the left menu (home and the libraries), the top bar with the search and
// who is signed in. Signed out, and in the player, only the page itself.
import { computed, ref, watch } from 'vue';
import { useRoute, useRouter } from 'vue-router';
import { pref, session, signOut, savePref } from './api';
import { lang, packs, setLang, t } from './i18n';
import Icon from './components/Icon.vue';
import { loadPanels } from './plugins';
import { dl, loadDlCount } from './downloads';
import { toastRun, toastState } from './toast';
import { cardSize } from './cardSize';
import { checkInvites, declineInvite, invites } from './together';
import { activity, cancelJob, poke } from './activity';
import Modal from './components/Modal.vue';
import { applyLook } from './look';
import SideList from './components/SideList.vue';
import ActivityPanel from './components/ActivityPanel.vue';

const route = useRoute();
const router = useRouter();
const framed = computed(() => !!session.me && !route.meta.bare && !route.meta.open);
const name = computed(() => (session.me && (session.me.display_name || session.me.username)) || '');
const serverName = computed(() => (session.status && session.status.name) || 'Lichtspiel');
watch(serverName, n => { document.title = n === 'Lichtspiel' ? n : `${n} · Lichtspiel`; }, { immediate: true });

const query = ref('');
// The box shows what the search page searches for (also when opened from an address), and empties elsewhere.
watch(() => [route.name, route.query.q], ([n, q]) => { if (n !== 'search') query.value = ''; else if (typeof q === 'string' && q !== query.value.trim()) query.value = q; }, { immediate: true });
let timer = 0;
// Typing searches after a short pause; Enter at once.
function search(now = false) {
  clearTimeout(timer);
  const go = () => {
    const q = query.value.trim();
    if (q) router.push({ name: 'search', query: { q } });
  };
  if (now) go(); else timer = window.setTimeout(go, 350);
}

// The plugins' own pages in the left menu (asked for again after signing in as someone else).
watch(() => session.me && session.me.id, id => { if (id) { loadPanels(); loadDlCount(); poke(); } }, { immediate: true });
// Invitations to watch together, looked for every 15 seconds.
window.setInterval(() => { if (session.me) checkInvites(); }, 15000);
function joinInvite() { const inv = invites.shown; invites.shown = null; if (inv) router.push('/together/' + inv.code); }

const menuOpen = ref(false);
// The left menu folded to its icons (kept in this browser, as it suits the screen); the ☰ in the top bar folds
// it. On a phone the ☰ opens the menu over the page instead.
const navSmall = ref((() => { try { return localStorage.getItem('vue-navSmall') === '1'; } catch { return false; } })());
function burger() {
  if (matchMedia('(max-width: 760px)').matches) { sideOpen.value = !sideOpen.value; return; }
  navSmall.value = !navSmall.value;
  try { localStorage.setItem('vue-navSmall', navSmall.value ? '1' : '0'); } catch { /* storage blocked */ }
}
const sideOpen = ref(false);
watch(() => route.fullPath, () => { sideOpen.value = false; menuOpen.value = false; });

function chooseLang(code: string) {
  setLang(code);
  savePref('lang', code).catch(() => {});
}
async function out() {
  await signOut();
  router.push({ name: 'login' });
}
// Signing out asks first; switching user goes straight to the sign-in (with the home profiles).
const askOut = ref(false);
// The look, as under Settings → Account (kept with the account).
const LOOKS: [string, string][] = [['system', 'System'], ['light', 'Light'], ['dark', 'Dark'], ['oled', 'OLED (pure black)']];
const ACCENTS: [string, string][] = [['rose', 'Rose'], ['amber', 'Amber'], ['blue', 'Blue'], ['green', 'Green'], ['red', 'Red'], ['violet', 'Violet']];
function setLook(key: string, value: string) { savePref(key, value).catch(() => {}); applyLook(); }
// The tooltip of the activity at the top: every line, with the file a scan is on.
// A click on the bar opens what runs, what waits and what is planned (ActivityPanel).
const actOpen = ref(false);
const activityTitle = () => activity.lines.map(l => l.text + (l.title ? '\n' + l.title : '')).join('\n') || activity.last;
// A page with tabs in its address stays while they change (no new loading); any other address is a new page.
const pageKey = computed(() => (route.name === 'server' || route.name === 'settings' ? String(route.name) : route.path));
const playingLabel = computed(() => (activity.playing ? t('Dashboard · {n} playing now', { n: activity.playing }) : t('Dashboard · nothing is playing')));
</script>

<template>
  <div v-if="framed" class="frame" :class="{ 'side-open': sideOpen, 'nav-small': navSmall }" :style="{ '--card-grid': cardSize + 'px' }">
    <header class="top">
      <button class="iconbtn burger" :aria-label="t('Menu')" :title="navSmall ? t('Expand the menu') : t('Collapse the menu to its icons')" @click="burger"><Icon name="menu" /></button>
      <RouterLink to="/" class="brand">
        <!-- The light beam of the classic app's logo. -->
        <svg viewBox="0 0 32 32" aria-hidden="true"><defs><linearGradient id="beam"><stop offset="0" style="stop-color: var(--accent)" stop-opacity=".95" /><stop offset="1" style="stop-color: var(--accent)" stop-opacity=".15" /></linearGradient></defs><path d="M4.5 16 27 4.5v23z" fill="url(#beam)" /><circle cx="4.5" cy="16" r="3.5" style="fill: var(--accent)" /><rect x="27.5" y="3.5" width="3.2" height="25" rx="1.6" style="fill: var(--accent)" /></svg>
        <span>{{ serverName }}</span>
      </RouterLink>
      <form class="search" role="search" @submit.prevent="search(true)">
        <Icon name="search" />
        <input v-model="query" type="search" :placeholder="t('Search')" :aria-label="t('Search')" @input="search()">
      </form>
      <div v-if="activity.busy || activity.last" class="activity" :class="{ open: actOpen }" :title="actOpen ? '' : activityTitle()" role="button" tabindex="0" :aria-expanded="actOpen" @click="!($event.target as Element).closest('button') && (actOpen = !actOpen)" @keydown.enter.self="actOpen = !actOpen">
        <template v-if="activity.busy">
          <span class="spin"></span>
          <template v-for="(l, i) in activity.lines" :key="i">
            <span class="txt">{{ l.text }}</span><span v-if="l.pct !== undefined" class="pbar"><i :style="{ width: l.pct + '%' }"></i></span>
          </template>
          <button v-if="activity.canCancel" class="small" @click.stop="cancelJob">{{ t('Cancel') }}</button>
        </template>
        <span v-else class="txt">{{ activity.last }}</span>
      </div>
      <Modal v-if="askOut" :title="t('Sign out on this device?')" @close="askOut = false">
        <template #actions><button class="primary" @click="askOut = false; out()">{{ t('Sign out') }}</button><button @click="askOut = false">{{ t('Cancel') }}</button></template>
      </Modal>
      <ActivityPanel v-if="actOpen" @close="actOpen = false" />
      <!-- The owner's way to the dashboard, with how many play now (also on phones, where the activity is hidden). -->
      <RouterLink v-if="session.me && session.me.admin" to="/dashboard" class="npbtn" :class="{ live: (activity.playing || 0) > 0 }" active-class="on"
        :title="playingLabel" :aria-label="playingLabel">
        <svg viewBox="0 0 24 24" aria-hidden="true"><path d="M2 12h4l3-8 6 16 3-8h4" /></svg><b v-if="activity.playing">{{ activity.playing }}</b>
      </RouterLink>
      <div class="who">
        <button class="userbtn" aria-haspopup="menu" :aria-expanded="menuOpen" @click="menuOpen = !menuOpen">
          <span class="av">{{ (name[0] || '?').toUpperCase() }}</span><span class="nm">{{ name }}</span>
        </button>
        <div v-if="menuOpen" class="menu" role="menu" @click.stop>
          <label class="menu-row">
            {{ t('Language') }}
            <select :value="lang" @change="chooseLang(($event.target as HTMLSelectElement).value)">
              <option v-for="(p, code) in packs" :key="code" :value="code">{{ p.name }}</option>
            </select>
          </label>
          <label class="menu-row">
            {{ t('Appearance') }}
            <select :value="pref('theme', 'system')" @change="setLook('theme', ($event.target as HTMLSelectElement).value)"><option v-for="[v, l] in LOOKS" :key="v" :value="v">{{ t(l) }}</option></select>
          </label>
          <label class="menu-row">
            {{ t('Accent color') }}
            <select :value="pref('accent', 'rose')" @change="setLook('accent', ($event.target as HTMLSelectElement).value)"><option v-for="[v, l] in ACCENTS" :key="v" :value="v">{{ t(l) }}</option></select>
          </label>
          <hr>
          <RouterLink class="menu-row" to="/settings" @click="menuOpen = false">{{ t('Settings') }}</RouterLink>
          <RouterLink class="menu-row" to="/settings/devices" @click="menuOpen = false">{{ t('Sign in a device') }}</RouterLink>
          <a class="menu-row" href="/">{{ t('Classic web app') }}</a>
          <hr>
          <button class="menu-row" role="menuitem" @click="menuOpen = false; out()">{{ t('Switch user') }}</button>
          <button class="menu-row" role="menuitem" @click="menuOpen = false; askOut = true">{{ t('Sign out') }}</button>
        </div>
      </div>
    </header>
    <nav class="side" :aria-label="t('Libraries')">
      <RouterLink to="/" class="nav" exact-active-class="on" :title="t('Home')"><Icon name="home" /><span>{{ t('Home') }}</span></RouterLink>
      <RouterLink to="/watchlist" class="nav" active-class="on" :title="t('Watchlist')"><Icon name="mark" /><span>{{ t('Watchlist') }}</span></RouterLink>
      <RouterLink v-if="session.me && session.me.can_download" to="/downloads" class="nav" active-class="on" :title="t('Downloads')"><Icon name="download" /><span>{{ t('Downloads') }}</span><small v-if="dl.busy" class="nav-count" :title="t('{n} being converted', { n: dl.busy })">{{ dl.busy }}</small></RouterLink>
      <SideList />
      <template v-if="session.me && session.me.admin">
        <div class="nav-head">{{ t('Server') }}</div>
        <RouterLink to="/dashboard" class="nav" active-class="on" :title="t('Dashboard')"><Icon name="dashboard" /><span>{{ t('Dashboard') }}</span></RouterLink>
        <RouterLink to="/server" class="nav" :class="{ on: route.name === 'server' }" active-class="on" :title="t('Server settings')"><Icon name="dns" /><span>{{ t('Server settings') }}</span></RouterLink>
        <RouterLink to="/libraries" class="nav" active-class="on" :title="t('Libraries')"><Icon name="folder" /><span>{{ t('Libraries') }}</span></RouterLink>
        <RouterLink to="/users" class="nav" active-class="on" :title="t('Users & invites')"><Icon name="people" /><span>{{ t('Users & invites') }}</span></RouterLink>
        <RouterLink to="/reports" class="nav" active-class="on" :title="t('Reported problems')"><Icon name="flag" /><span>{{ t('Reported problems') }}</span></RouterLink>
        <RouterLink to="/plugins" class="nav" active-class="on" :title="t('Plugins')"><Icon name="extension" /><span>{{ t('Plugins') }}</span></RouterLink>
      </template>
      <div class="nav-head"></div>
      <RouterLink to="/settings" class="nav" active-class="on" :title="t('Settings')"><Icon name="settings" /><span>{{ t('Settings') }}</span></RouterLink>
    </nav>
    <div v-if="sideOpen" class="scrim" @click="sideOpen = false"></div>
    <main class="main" @click="menuOpen = false">
      <RouterView :key="pageKey" />
    </main>
    <!-- Phones: a bar at the bottom instead of the side menu; "Menu" opens the side menu with the rest. -->
    <nav class="tabbar" :aria-label="t('Menu')">
      <RouterLink to="/" exact-active-class="on"><Icon name="home" /><span>{{ t('Home') }}</span></RouterLink>
      <RouterLink to="/watchlist" active-class="on"><Icon name="mark" /><span>{{ t('Watchlist') }}</span></RouterLink>
      <RouterLink to="/playlists" :class="{ on: route.name === 'playlist' }" active-class="on"><Icon name="playlist" /><span>{{ t('Playlists') }}</span></RouterLink>
      <button type="button" :class="{ on: sideOpen }" :aria-expanded="sideOpen" @click="sideOpen = !sideOpen"><Icon name="menu" /><span>{{ t('Menu') }}</span></button>
    </nav>
  </div>
  <!-- (Keyed by the address: the next episode is a new player.) -->
  <RouterView v-else :key="route.path" />
  <div v-if="toastState.text" :key="toastState.n" class="toast" role="status">{{ toastState.text }}<button v-if="toastState.action" class="small toast-act" @click="toastRun">{{ toastState.action.label }}</button></div>
  <Modal v-if="invites.shown" :title="t('Watch together')" @close="invites.shown = null">
    <p>{{ t('{by} asks you to watch “{title}” together.', { by: invites.shown.by, title: invites.shown.title }) }}</p>
    <p v-if="invites.shown.members.some(n => n !== invites.shown!.by)" class="dim">{{ t('Also there: {names}', { names: invites.shown.members.filter(n => n !== invites.shown!.by).join(', ') }) }}</p>
    <template #actions>
      <button class="primary" @click="joinInvite">{{ t('▶ Join') }}</button>
      <button @click="declineInvite(invites.shown)">{{ t('No, thanks') }}</button>
    </template>
  </Modal>
</template>
