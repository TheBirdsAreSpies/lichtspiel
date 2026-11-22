// The pages. Hash addresses (/vue/#/item/12), so the server only has to serve /vue/ itself.
import { createRouter, createWebHashHistory } from 'vue-router';
import { session } from './api';

export const router = createRouter({
  history: createWebHashHistory(),
  routes: [
    { path: '/', name: 'home', component: () => import('./views/HomeView.vue') },
    { path: '/login', name: 'login', component: () => import('./views/LoginView.vue'), meta: { open: true } },
    { path: '/login/code', name: 'code', component: () => import('./views/AuthView.vue'), meta: { open: true, signedOut: true } },
    { path: '/forgot', name: 'forgot', component: () => import('./views/AuthView.vue'), meta: { open: true, signedOut: true } },
    { path: '/reset/:code', name: 'reset', component: () => import('./views/AuthView.vue'), meta: { open: true } },
    { path: '/invite/:code', name: 'invite', component: () => import('./views/AuthView.vue'), meta: { open: true, signedOut: true } },
    { path: '/downloads', name: 'downloads', component: () => import('./views/DownloadsView.vue') },
    { path: '/dashboard', name: 'dashboard', component: () => import('./views/DashboardView.vue'), meta: { admin: true } },
    { path: '/users', name: 'users', component: () => import('./views/UsersView.vue'), meta: { admin: true } },
    { path: '/plugins', name: 'plugins', component: () => import('./views/PluginsView.vue'), meta: { admin: true } },
    { path: '/plugin/:id', name: 'plugin', component: () => import('./views/PluginPageView.vue'), props: true },
    { path: '/link/:code?', name: 'link', component: () => import('./views/LinkView.vue'), props: true },
    { path: '/library/:id', name: 'library', component: () => import('./views/LibraryView.vue'), props: true },
    { path: '/item/:id', name: 'item', component: () => import('./views/ItemView.vue'), props: true },
    { path: '/search', name: 'search', component: () => import('./views/SearchView.vue') },
    { path: '/person/:id', name: 'person', component: () => import('./views/PersonView.vue'), props: true },
    { path: '/settings/:tab?', name: 'settings', component: () => import('./views/SettingsView.vue'), props: true },
    { path: '/watchlist', name: 'watchlist', component: () => import('./views/WatchlistView.vue') },
    { path: '/playlists', name: 'playlists', component: () => import('./views/PlaylistsView.vue') },
    { path: '/playlist/:id', name: 'playlist', component: () => import('./views/PlaylistView.vue'), props: true },
    { path: '/play/:id', name: 'play', component: () => import('./views/PlayerView.vue'), props: true, meta: { bare: true } },
    { path: '/:rest(.*)', redirect: '/' },
  ],
  scrollBehavior: (_to, _from, saved) => saved || { top: 0 },
});

// Signed out: every page but the sign-in leads there, and back after signing in.
router.beforeEach(to => {
  if (!session.me && !to.meta.open) return { name: 'login', query: to.fullPath !== '/' ? { next: to.fullPath } : {} };
  if (session.me && (to.name === 'login' || to.meta.signedOut)) return { name: 'home' };
  if (to.meta.admin && !(session.me && session.me.admin)) return { name: 'home' };
});

// A part of the app that is no longer there: the server got a new version while this page was open (its
// files have new names). Loading the page again gets the new version; once a minute at most, so a part
// that is really missing does not reload the page over and over.
router.onError((err, to) => {
  if (!/dynamically imported module|Importing a module script failed|error loading dynamically/i.test(String(err && (err as Error).message))) return;
  try {
    const last = Number(sessionStorage.getItem('vue-reloaded') || 0);
    if (Date.now() - last < 60000) return;
    sessionStorage.setItem('vue-reloaded', String(Date.now()));
  } catch { /* storage blocked: try once anyway */ }
  window.location.hash = '#' + to.fullPath;
  window.location.reload();
});
