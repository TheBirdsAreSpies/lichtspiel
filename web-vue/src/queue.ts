// What plays after the current video: the rest of a playlist ("Play all", "Shuffle", ▶ on one of its rows).
// The player goes on with it at the end instead of with the next episode. Starting anything else empties it
// (the player opens with ?queue=1 only when it comes from here).
import { reactive } from 'vue';
import type { Router } from 'vue-router';
import { get, list, type Item } from './api';
import { t } from './i18n';
import { toast } from './toast';
import { resumeAt } from './format';

export const queue = reactive({ items: [] as Item[] });

/** How many videos in a row went on by themselves, without anyone touching anything (the player asks
 * "Are you still watching?" after a few), and the sleep timer (at = when it pauses, end = at the end of the
 * title). Both last from one video to the next. */
export const watching = reactive({ autoRun: 0, sleep: null as { at?: number, end?: boolean } | null });

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

// ---- "Add to queue": what plays after whatever is playing now, one after the other. Kept in this browser
// under the classic app's key ("queue"), so both apps share it. The player takes from it after a playlist's
// rest and before the next episode.
export type Queued = Pick<Item, 'id' | 'type' | 'title' | 'grandparentTitle' | 'parentIndex' | 'index' | 'thumb' | 'duration'>;
export const upQueue = reactive({ list: ((): Queued[] => { try { return JSON.parse(localStorage.getItem('queue') || '[]'); } catch { return []; } })() });
export function saveQueue() { try { localStorage.setItem('queue', JSON.stringify(upQueue.list)); } catch { /* private window */ } }
// (Another tab changed it.)
window.addEventListener('storage', e => { if (e.key === 'queue') { try { upQueue.list = JSON.parse(e.newValue || '[]'); } catch { upQueue.list = []; } } });

/** Adds a title (a show or season: all its episodes) to the end of the queue; says how many. */
export async function addToQueue(it: Item) {
  const leaves = ['movie', 'episode', 'clip', 'track'].includes(it.type) ? [it]
    : list<Item>(await get(`/library/metadata/${it.id}/allLeaves`).catch(() => null));
  const ok = leaves.filter(x => !x.missing);
  if (!ok.length) { toast(t('Nothing to add')); return; }
  upQueue.list.push(...ok.map(x => ({ id: x.id, type: x.type, title: x.title, grandparentTitle: x.grandparentTitle, parentIndex: x.parentIndex, index: x.index, thumb: x.thumb, duration: x.duration })));
  saveQueue();
  toast(ok.length === 1 ? t('Added to the queue') : t('{n} episodes added to the queue', { n: ok.length }));
}
export const queueTitle = (x: Queued) => (x.type === 'episode' ? `${x.grandparentTitle || ''} · ${t('S{n}', { n: x.parentIndex || 0 })}${t('E{n}', { n: x.index || 0 })} ${x.title}` : x.title);
