// The look of the app (Settings → Account → This app): light, dark or pure black (or the device's choice),
// and the accent colour. Kept with the account and, for the first paint without a flash, in this browser
// under the classic app's keys ("theme", "accent"; index.html reads them before the app starts).
import { pref, session } from './api';
import { watch } from 'vue';

const mq = typeof matchMedia === 'function' ? matchMedia('(prefers-color-scheme: light)') : null;
export function applyLook() {
  const read = (k: string, d: string) => { try { return localStorage.getItem(k) || d; } catch { return d; } };
  const theme = pref('theme', read('theme', 'system')), accent = pref('accent', read('accent', 'rose'));
  try { localStorage.setItem('theme', theme); localStorage.setItem('accent', accent); } catch { /* storage blocked */ }
  const d = document.documentElement;
  d.dataset.theme = theme === 'oled' ? 'oled' : theme === 'light' || (theme !== 'dark' && mq && mq.matches) ? 'light' : 'dark';
  d.dataset.accent = accent;
  const meta = document.querySelector('meta[name="color-scheme"]');
  if (meta) meta.setAttribute('content', d.dataset.theme === 'light' ? 'light' : 'dark');
}
mq?.addEventListener?.('change', applyLook);
// Signed in (or the account's choice changed elsewhere in the app): its look.
watch(() => [session.me?.prefs?.theme, session.me?.prefs?.accent], applyLook);
