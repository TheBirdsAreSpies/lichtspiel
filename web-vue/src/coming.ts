// Coming soon (GET /api/upcoming): the next episodes of the shows in the libraries (looked up once a day)
// and what Radarr and Sonarr expect (through the wish list plugin). A row on the home page and the calendar.
import { get } from './api';
import { lang, t } from './i18n';

export interface Coming {
  kind: 'episode' | 'movie' | string, show_id?: number, title: string, year?: number, season?: number, episode?: number,
  name?: string, date: string, poster?: string, thumb?: string, note?: string, source?: string,
}
export const loadComing = (days: number) => get<{ today: string, entries: Coming[] }>('/api/upcoming?days=' + days);

const NOTES: Record<string, string> = { 'in cinemas': 'In cinemas', 'digital release': 'Digital release', 'on disc': 'On disc' };
/** "2026-11-28" → Today / Tomorrow / the weekday within a week / "Fri, 28.11." later; long: with the full date. */
export function comingDay(date: string, long = false): string {
  const [y, m, d] = date.split('-').map(Number), at = new Date(y, m - 1, d), now = new Date();
  const days = Math.round((at.getTime() - new Date(now.getFullYear(), now.getMonth(), now.getDate()).getTime()) / 86400000);
  const full = at.toLocaleDateString(lang.value, long ? { weekday: 'long', day: 'numeric', month: 'long' } : { weekday: 'short', day: 'numeric', month: 'numeric' });
  if (days === 0) return long ? t('Today') + ' · ' + full : t('Today');
  if (days === 1) return long ? t('Tomorrow') + ' · ' + full : t('Tomorrow');
  if (days < 7 && !long) return at.toLocaleDateString(lang.value, { weekday: 'long' });
  return full;
}
/** "S2 E5 · Name" for an episode, "2026 · In cinemas" for a movie. */
export const comingWhat = (e: Coming) => (e.kind === 'episode'
  ? [e.season !== undefined && e.episode !== undefined ? `${t('S{n}', { n: e.season })} ${t('E{n}', { n: e.episode })}` : '', e.name].filter(Boolean).join(' · ')
  : [e.year, e.note ? t(NOTES[e.note] || e.note) : ''].filter(Boolean).join(' · '));
