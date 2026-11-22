// The pages. Hash addresses (/vue/#/item/12), so the server only has to serve /vue/ itself.
import { createRouter, createWebHashHistory } from 'vue-router';
import { session } from './api';

export const router = createRouter({
  history: createWebHashHistory(),
  routes: [
    { path: '/', name: 'home', component: () => import('./views/HomeView.vue') },
    { path: '/login', name: 'login', component: () => import('./views/LoginView.vue'), meta: { open: true } },
    { path: '/library/:id', name: 'library', component: () => import('./views/LibraryView.vue'), props: true },
    { path: '/item/:id', name: 'item', component: () => import('./views/ItemView.vue'), props: true },
    { path: '/search', name: 'search', component: () => import('./views/SearchView.vue') },
    { path: '/play/:id', name: 'play', component: () => import('./views/PlayerView.vue'), props: true, meta: { bare: true } },
    { path: '/:rest(.*)', redirect: '/' },
  ],
  scrollBehavior: (_to, _from, saved) => saved || { top: 0 },
});

// Signed out: every page but the sign-in leads there, and back after signing in.
router.beforeEach(to => {
  if (!session.me && !to.meta.open) return { name: 'login', query: to.fullPath !== '/' ? { next: to.fullPath } : {} };
  if (session.me && to.name === 'login') return { name: 'home' };
});
