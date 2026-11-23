import { createApp } from 'vue';
import App from './App.vue';
import { router } from './router';
import { useStagger } from './motion';
import { boot, session, setSignedOutHandler } from './api';
import { setLang, packs } from './i18n';
import './style.css';
import { applyLook } from './look';

useStagger(router);
setSignedOutHandler(() => { router.push({ name: 'login' }); });

// Who is signed in first, so the first page is the right one (and in the language kept with the account).
boot().finally(() => {
  const saved = session.me && session.me.prefs && session.me.prefs.lang;
  if (saved && packs[saved]) setLang(saved);
  applyLook();
  createApp(App).use(router).mount('#app');
});
