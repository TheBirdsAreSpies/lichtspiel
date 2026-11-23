// The left menu's list: the libraries, Playlists, Statistics and the plugins' pages, in the order each person
// chose (kept on the server per person, PUT /api/me/sidebar, shared with the classic app); unpinned ones sit
// under "More". New entries come after the ones already placed, in the server's order.
import { computed } from 'vue';
import { api, session, type Section } from './api';
import { t } from './i18n';
import { panels } from './plugins';
import { toast } from './toast';

export interface NavEntry { key: string, title: string, to: string, icon: string, sec?: Section, hidden: boolean }
const secIcon = (type: string) => ({ movie: 'movie', show: 'tv', artist: 'music', music: 'music', photo: 'photo' }[type] || 'folder');

export const navList = computed<NavEntry[]>(() => {
  const all: Omit<NavEntry, 'hidden'>[] = [
    ...session.sections.map(s => ({ key: 's' + s.id, title: s.title, to: '/library/' + s.id, icon: secIcon(s.type), sec: s })),
    { key: 'playlists', title: t('Playlists'), to: '/playlists', icon: 'playlist' },
    { key: 'stats', title: t('Statistics'), to: '/stats', icon: 'chart' },
    ...panels.list.filter(p => p.sidebar).map(p => ({ key: 'p:' + p.id, title: p.name, to: '/plugin/' + p.id, icon: 'extension' })),
  ];
  const sb = session.me?.sidebar || {};
  const order = sb.order || [], hidden = new Set(sb.hidden || []);
  const pos = new Map(all.map((e, i) => [e.key, order.includes(e.key) ? order.indexOf(e.key) : order.length + i]));
  return all.sort((a, b) => pos.get(a.key)! - pos.get(b.key)!).map(e => ({ ...e, hidden: hidden.has(e.key) }));
});

export async function putNav(sb: { order: string[], hidden: string[] }) {
  if (!session.me) return;
  session.me.sidebar = sb;
  try { await api('PUT', '/api/me/sidebar', sb); } catch (e) { toast((e as Error).message, 5000); }
}
export const saveNav = (list: NavEntry[]) => putNav({ order: list.map(e => e.key), hidden: list.filter(e => e.hidden).map(e => e.key) });

/** Pinned again: after the last pinned entry. Unpinned: at the end (under "More"). */
export function setPinned(key: string, pinned: boolean) {
  const list = navList.value.map(e => ({ ...e }));
  const e = list.find(x => x.key === key); if (!e) return;
  list.splice(list.indexOf(e), 1);
  e.hidden = !pinned;
  if (pinned) { let at = 0; list.forEach((x, i) => { if (!x.hidden) at = i + 1; }); list.splice(at, 0, e); } else list.push(e);
  saveNav(list);
  toast(pinned ? t('Pinned to the menu') : t('Unpinned — it is under “More” in the menu'));
}
