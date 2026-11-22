// Library filters (GET /library/sections/{id}/filters): their names, how a value is shown, and how they sit
// in an address (?f.genre=Drama). The values are asked for once a minute at most: they cover the whole
// library whatever is filtered.
import { get, list } from './api';
import { langName } from './format';
import { t } from './i18n';

export interface FilterValue { value: string, count: number }
export interface Filter { filter: string, type: string, count?: number | null, values: FilterValue[], advanced?: boolean }

const NAMES: Record<string, string> = { genre: 'Genre', year: 'Year', decade: 'Decade', content_rating: 'Age rating', collection: 'Collection', director: 'Director',
  actor: 'Actor', writer: 'Writer', producer: 'Producer', country: 'Country', studio: 'Studio', network: 'Network', video_resolution: 'Resolution', hdr: 'HDR',
  dovi: 'Dolby Vision', hdr10plus: 'HDR10+', atmos: 'Dolby Atmos', unwatched: 'Unwatched', in_progress: 'In progress', unwatched_leaves: 'Unwatched',
  unmatched: 'Unmatched', video_codec: 'Video codec', audio_codec: 'Audio codec', subtitle_codec: 'Subtitle format', audio_layout: 'Audio channels',
  audio_language: 'Audio language', subtitle_language: 'Subtitle language', edition: 'Edition', label: 'Label', folder: 'Folder', duplicate: 'Duplicates' };
const RES: Record<string, string> = { '4k': '4K', 1080: '1080p', 720: '720p', 480: '480p', sd: 'SD' };

/** The groups of the panel: states first, then the usual lists, then the rest. */
export const STATE = ['unwatched', 'unwatched_leaves', 'in_progress', 'unmatched', 'duplicate'];
export const MAIN = ['genre', 'year', 'decade', 'content_rating', 'collection', 'director', 'actor', 'writer', 'producer', 'country', 'network', 'studio'];

export const filterName = (f: string) => t(NAMES[f] || f);
export function valueName(f: string, v: string): string {
  if (f === 'video_resolution') return RES[v] || v;
  if (f === 'decade') return t('{v}s', { v });
  if (/_language$/.test(f)) return langName(v) || v;
  return v;
}
/** What a chip says: "Unwatched", "Unwatched: no", "Genre: Drama". */
export function chipLabel(f: string, v: string): string {
  if (v === 'true') return filterName(f);
  if (v === 'false') return t('{name}: no', { name: filterName(f) });
  return `${filterName(f)}: ${valueName(f, v)}`;
}

/** The filters in an address's query: { genre: 'Drama' } from ?f.genre=Drama. */
export function fromQuery(q: Record<string, unknown>): Record<string, string> {
  const out: Record<string, string> = {};
  for (const [k, v] of Object.entries(q)) if (k.startsWith('f.') && typeof v === 'string' && v) out[k.slice(2)] = v;
  return out;
}

const cache = new Map<string, { at: number, p: Promise<Filter[]> }>();
export function loadFilters(url: string): Promise<Filter[]> {
  const hit = cache.get(url);
  if (hit && Date.now() - hit.at < 60000) return hit.p;
  const p = get(url).then(j => list<Filter>(j, 'Filter'));
  cache.set(url, { at: Date.now(), p });
  p.catch(() => cache.delete(url));
  return p;
}

/** The fields and operators the server's rules know (GET /api/filters): smart playlists and the advanced
 * library filters use them. Asked for once per page load. */
export interface RuleField { field: string, label: string, type: 'text' | 'number' | 'date' | 'bool' | 'sort' }
export interface Rule { field: string, op: string, value: unknown }
let fieldsP: Promise<{ fields: RuleField[], operators: { op: string, label: string }[] }> | null = null;
export function ruleFields() {
  if (!fieldsP) { fieldsP = get('/api/filters'); fieldsP.catch(() => { fieldsP = null; }); }
  return fieldsP;
}
