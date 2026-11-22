// What plays after the current video: the rest of a playlist ("Play all", "Shuffle", ▶ on one of its rows).
// The player goes on with it at the end instead of with the next episode. Starting anything else empties it
// (the player opens with ?queue=1 only when it comes from here).
import { reactive } from 'vue';
import type { Router } from 'vue-router';
import type { Item } from './api';
import { resumeAt } from './format';

export const queue = reactive({ items: [] as Item[] });

/** Plays the first of `items` and keeps the rest for afterwards. */
export function playAll(router: Router, items: Item[]) {
  const [first, ...rest] = items;
  if (!first) return;
  queue.items = rest;
  router.push({ name: 'play', params: { id: first.id }, query: { t: String(Math.floor(resumeAt(first))), queue: '1' } });
}

/** The next one from the queue (taken off it), or null. */
export function nextInQueue(): Item | null {
  return queue.items.shift() || null;
}

/** Random order that does not play the same show twice in a row while others are left. */
export function smartShuffle(items: Item[]): Item[] {
  const left = items.slice();
  for (let i = left.length - 1; i > 0; i--) { const j = Math.floor(Math.random() * (i + 1)); [left[i], left[j]] = [left[j], left[i]]; }
  const group = (it: Item) => it.grandparentRatingKey || null;
  const out: Item[] = [];
  while (left.length) {
    const last = out.length ? group(out[out.length - 1]) : null;
    let k = last ? left.findIndex(it => group(it) !== last) : 0;
    if (k < 0) k = 0;
    out.push(left.splice(k, 1)[0]);
  }
  return out;
}
