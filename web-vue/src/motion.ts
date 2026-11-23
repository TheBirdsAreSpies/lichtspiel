// The posters in view come in one after the other (row by row, left to right) when a page is opened, as in
// the classic app (stagger() in web/app.js); the rest are simply there. Not when going back, and not for
// people who asked their system for less motion. A page that draws its posters a moment later (a library
// waiting for its first titles) still gets it: we watch for them for 1.5 seconds after opening.
import type { Router } from 'vue-router';

const calm = () => !window.matchMedia || matchMedia('(prefers-reduced-motion: reduce)').matches;
const fadeIn = (el: Element, delay: number) => el.animate([{ opacity: 0, transform: 'translateY(14px)', offset: 0 }],
  { duration: 320, delay, easing: 'cubic-bezier(.2,.8,.2,1)', fill: 'backwards' });

let back = false;
let watching: MutationObserver | null = null;
let until = 0;

function tryNow(): boolean {
  const main = document.querySelector('.main'); if (!main) return false;
  const W = innerWidth, H = innerHeight;
  const seen = [...main.querySelectorAll('.card:not(.ph)')].filter(c => {
    const r = c.getBoundingClientRect(); return r.width && r.top < H && r.bottom > 0 && r.left < W && r.right > 0;
  });
  if (!seen.length) return false;
  // All in view, however many a big screen shows; with many the steps get shorter, so the last is in within a second.
  const step = Math.min(28, 900 / seen.length);
  seen.forEach((c, i) => fadeIn(c, 60 + i * step));
  return true;
}

function stop() { if (watching) { watching.disconnect(); watching = null; } }

export function useStagger(router: Router) {
  window.addEventListener('popstate', () => { back = true; });
  router.afterEach((to, from, failure) => {
    const wasBack = back; back = false;
    stop();
    if (failure || wasBack || calm() || to.path === from.path && JSON.stringify(to.query) === JSON.stringify(from.query)) return;
    until = Date.now() + 1500;
    // Once the page has settled (nothing changed in it for a moment: a library draws its posters twice while
    // it starts), so the motion is not lost on cards that are replaced right after.
    const main = document.querySelector('.main'); if (!main) return;
    let quiet = 0;
    const settle = () => {
      clearTimeout(quiet);
      quiet = window.setTimeout(() => { if (watching && (Date.now() > until || tryNow())) stop(); }, 90);
    };
    const mine = new MutationObserver(settle);
    watching = mine;
    mine.observe(main, { childList: true, subtree: true });
    settle();
    window.setTimeout(() => { if (watching === mine) stop(); }, 1700);
  });
}
