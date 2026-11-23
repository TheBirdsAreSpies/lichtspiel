// The size of the posters in the grids (the slider in a library's bar), on wide screens. Kept as the classic
// app keeps it ("cardSize": in this browser and in the account's settings), so both apps show the same size.
import { computed, ref } from 'vue';
import { api, pref, session } from './api';

export const CARD = { min: 100, max: 300, def: 160 };
const clamp = (v: number) => Math.max(CARD.min, Math.min(CARD.max, Math.round(v)));
const stored = () => { try { return Number(localStorage.getItem('cardSize')) || 0; } catch { return 0; } };

// (The account's value wins at the start; the slider's while it is moved.)
const moved = ref(0);
export const cardSize = computed(() => clamp(moved.value || Number(pref('cardSize')) || stored() || CARD.def));

let timer = 0;
export function setCardSize(v: number) {
  const n = clamp(v);
  moved.value = n;
  try { localStorage.setItem('cardSize', String(n)); } catch { /* storage blocked */ }
  const me = session.me;
  if (!me) return;
  me.prefs = { ...(me.prefs || {}), cardSize: String(n) };
  clearTimeout(timer);
  timer = window.setTimeout(() => { api('POST', '/api/me/prefs', { cardSize: String(n) }).catch(() => {}); }, 500);
}
