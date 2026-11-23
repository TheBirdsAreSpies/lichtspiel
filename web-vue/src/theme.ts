// Theme music on a movie's or show's page, as in the classic app: quiet, faded out when leaving the page,
// switched off under Settings → Playback ("themeMusic"), once or over and over ("themeLoop"), and muted with
// the button on the page (kept in this browser, "themeMuted", the classic app's keys).
import { reactive } from 'vue';
import { pref, ticket, withTicket } from './api';

const VOLUME = 0.5, FADE_MS = 2000;
export const theme = reactive({ playing: false, muted: (() => { try { return localStorage.getItem('themeMuted') === '1'; } catch { return false; } })() });
let audio: HTMLAudioElement | null = null;
let forId = '';

function fade(a: HTMLAudioElement) {
  const from = a.volume, start = Date.now();
  const step = () => {
    const v = from - (from * (Date.now() - start)) / FADE_MS;
    if (v > 0) { a.volume = v; setTimeout(step, 100); } else { a.pause(); a.removeAttribute('src'); }
  };
  step();
}

/** Starts the theme of an item's page (once per page: a second call for the same id does nothing). */
export async function startTheme(id: string) {
  if (pref('themeMusic', '1') !== '1' || forId === id) return;
  stopTheme();
  forId = id;
  const tk = await ticket('item=' + id).catch(() => '');
  if (!tk || forId !== id) { if (forId === id) forId = ''; return; }
  const a = new Audio(withTicket(`/library/metadata/${id}/theme`, tk));
  a.volume = VOLUME; a.loop = pref('themeLoop', '1') === '1'; a.muted = theme.muted;
  audio = a;
  a.addEventListener('error', () => { if (audio === a) { audio = null; theme.playing = false; } });
  a.addEventListener('ended', () => { if (audio === a) { audio = null; theme.playing = false; } });
  a.addEventListener('playing', () => { if (audio === a) theme.playing = true; });
  a.play().catch(e => {
    // The browser wants a click first (a page opened by a link or reloaded): the first click or key starts it.
    if (!e || (e as Error).name !== 'NotAllowedError') return;
    const go = () => {
      document.removeEventListener('pointerdown', go, true); document.removeEventListener('keydown', go, true);
      if (audio === a) a.play().catch(() => {});
    };
    document.addEventListener('pointerdown', go, true); document.addEventListener('keydown', go, true);
  });
}

/** Fades out what plays (leaving the page, or playing the title). */
export function stopTheme() {
  const a = audio; audio = null; forId = ''; theme.playing = false;
  if (a) fade(a);
}

export function toggleThemeMute() {
  theme.muted = !theme.muted;
  try { localStorage.setItem('themeMuted', theme.muted ? '1' : '0'); } catch { /* storage blocked */ }
  if (audio) audio.muted = theme.muted;
}
