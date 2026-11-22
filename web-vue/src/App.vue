<script setup lang="ts">
// The frame around every page: the left menu (home and the libraries), the top bar with the search and
// who is signed in. Signed out, and in the player, only the page itself.
import { computed, ref, watch } from 'vue';
import { useRoute, useRouter } from 'vue-router';
import { session, signOut, savePref } from './api';
import { lang, packs, setLang, t } from './i18n';
import Icon from './components/Icon.vue';
import { loadPanels, panels } from './plugins';
import { toastState } from './toast';

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
watch(() => session.me && session.me.id, id => { if (id) loadPanels(); }, { immediate: true });

const menuOpen = ref(false);
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
const icon = (type: string) => ({ movie: 'movie', show: 'tv', artist: 'music', photo: 'photo' }[type] || 'folder');
</script>

<template>
  <div v-if="framed" class="frame" :class="{ 'side-open': sideOpen }">
    <header class="top">
      <button class="iconbtn burger" :aria-label="t('Menu')" @click="sideOpen = !sideOpen"><Icon name="menu" /></button>
      <RouterLink to="/" class="brand">{{ serverName }}</RouterLink>
      <form class="search" role="search" @submit.prevent="search(true)">
        <Icon name="search" />
        <input v-model="query" type="search" :placeholder="t('Search')" :aria-label="t('Search')" @input="search()">
      </form>
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
          <RouterLink class="menu-row" to="/settings">{{ t('Settings') }}</RouterLink>
          <a class="menu-row" href="/">{{ t('Classic web app') }}</a>
          <button class="menu-row" role="menuitem" @click="out">{{ t('Sign out') }}</button>
        </div>
      </div>
    </header>
    <nav class="side" :aria-label="t('Libraries')">
      <RouterLink to="/" class="nav" exact-active-class="on"><Icon name="home" /><span>{{ t('Home') }}</span></RouterLink>
      <RouterLink to="/watchlist" class="nav" active-class="on"><Icon name="mark" /><span>{{ t('Watchlist') }}</span></RouterLink>
      <RouterLink to="/playlists" class="nav" :class="{ on: route.name === 'playlist' }" active-class="on"><Icon name="playlist" /><span>{{ t('Playlists') }}</span></RouterLink>
      <div class="nav-head">{{ t('Libraries') }}</div>
      <RouterLink v-for="s in session.sections" :key="s.id" :to="'/library/' + s.id" class="nav" active-class="on">
        <Icon :name="icon(s.type)" /><span>{{ s.title }}</span>
      </RouterLink>
      <template v-if="panels.list.some(p => p.sidebar)">
        <div class="nav-head"></div>
        <RouterLink v-for="p in panels.list.filter(x => x.sidebar)" :key="p.id" :to="'/plugin/' + p.id" class="nav" active-class="on"><Icon name="extension" /><span>{{ p.name }}</span></RouterLink>
      </template>
      <template v-if="session.me && session.me.admin">
        <div class="nav-head">{{ t('Server') }}</div>
        <RouterLink to="/users" class="nav" active-class="on"><Icon name="people" /><span>{{ t('Users & invites') }}</span></RouterLink>
        <RouterLink to="/plugins" class="nav" active-class="on"><Icon name="extension" /><span>{{ t('Plugins') }}</span></RouterLink>
      </template>
      <div class="nav-head"></div>
      <RouterLink to="/settings" class="nav" active-class="on"><Icon name="settings" /><span>{{ t('Settings') }}</span></RouterLink>
    </nav>
    <div v-if="sideOpen" class="scrim" @click="sideOpen = false"></div>
    <main class="main" @click="menuOpen = false">
      <RouterView :key="route.path" />
    </main>
  </div>
  <!-- (Keyed by the address: the next episode is a new player.) -->
  <RouterView v-else :key="route.path" />
  <div v-if="toastState.text" :key="toastState.n" class="toast" role="status">{{ toastState.text }}</div>
</template>
