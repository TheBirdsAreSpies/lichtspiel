// Talking to the server's JSON API (GET /api/docs lists it). The sign-in is kept under the same keys as
// the usual web app's (localStorage / sessionStorage "token"), so both apps on one server share it.
import { reactive, ref } from 'vue';
import { t } from './i18n';

export interface Stream { kind: string, index?: number, codec?: string, language?: string, channels?: number, title?: string }
export interface Subtitle { key: string, language?: string, title?: string, forced?: boolean, sdh?: boolean, codec?: string, embedded?: boolean, file?: string, offsetMs?: number | null }
export interface Media {
  id?: number, height?: number, width?: number, size?: number, container?: string, videoCodec?: string, audioCodec?: string,
  audioChannels?: number, edition?: string, streams?: Stream[], subtitles?: Subtitle[], Part?: { duration?: number }[],
}
export interface Person { key?: string, name?: string, role?: string, thumb?: string, photo?: string }
export interface Item {
  id: number, type: string, title: string, ratingKey?: string, year?: number, thumb?: string, summary?: string, tagline?: string,
  duration?: number, viewOffset?: number, viewCount?: number, leafCount?: number, viewedLeafCount?: number,
  index?: number, parentIndex?: number, parentTitle?: string, grandparentTitle?: string, parentRatingKey?: string,
  grandparentRatingKey?: string, grandparentThumb?: string, genres?: string[], contentRating?: string, rating?: number,
  studio?: string, editionTitle?: string, missing?: boolean, librarySectionID?: number, squareThumb?: string, role?: string, favorite?: boolean, watchlist?: boolean, updatedAt?: number,
  credits?: number[], intro?: { start: number, end: number }, recap?: { start: number, end: number }, loudness?: number,
  /** "What happened so far": the episode before, played from `from` to `to` (ms, its credits). */
  catchUp?: { id: number, from: number, to: number, parentIndex?: number, index?: number, title?: string },
  Media?: Media[], extra?: { cast?: Person[], directors?: string[], writers?: string[], creators?: string[], original_title?: string, collections?: string[] },
}
export interface Playlist {
  id: number, title: string, summary?: string, kind: 'manual' | 'smart' | 'default', smart?: boolean, default?: boolean,
  leafCount: number, duration?: number, thumbs?: string[], owner?: string, shared?: boolean, pinned?: boolean, editable?: boolean,
  /** A smart playlist's rules. */
  query?: { match_all: boolean, rules: { field: string, op: string, value: unknown }[], sort: string, limit: number | null, sections: number[] },
}
/** A collection or category of a library (GET /library/sections/{id}/collections). */
export interface Group { title: string, thumb?: string, childCount: number, viewedLeafCount?: number }
export interface Hub { hubIdentifier: string, title: string, Metadata?: Item[] }
export interface Section { id: number, key: string, title: string, type: string, count?: number }
export interface Me {
  id: number, username: string, display_name?: string, full_name?: string, email?: string, two_factor?: boolean, admin: boolean,
  prefs?: Record<string, string>, can_download?: boolean, allow_transcode?: boolean,
}
export interface Status { name: string, version: string, build?: string, setup_needed: boolean, mail?: boolean, lan_url?: string }
export type Container<K extends string, T> = { MediaContainer: { [k in K]?: T[] } & { totalSize?: number, size?: number } };

const keepSignedIn = () => { try { return localStorage.getItem('autoLogin') !== '0'; } catch { return true; } };
function loadToken() { try { return sessionStorage.getItem('token') || localStorage.getItem('token') || ''; } catch { return ''; } }

/** Who is signed in and what they may see; filled by boot(). */
export const session = reactive({
  token: loadToken(),
  me: null as Me | null,
  status: null as Status | null,
  sections: [] as Section[],
});

export function saveToken(tok: string) {
  session.token = tok;
  try {
    if (keepSignedIn()) { localStorage.setItem('token', tok); sessionStorage.removeItem('token'); }
    else { sessionStorage.setItem('token', tok); localStorage.removeItem('token'); }
  } catch { /* private window: the sign-in lasts for this page */ }
}

export function forgetToken() {
  session.token = ''; session.me = null; session.sections = [];
  tickets.clear(); pictureTicket.value = '';
  try { localStorage.removeItem('token'); sessionStorage.removeItem('token'); } catch { /* none */ }
}

export class ApiError extends Error {
  constructor(message: string, public status: number, public data: Record<string, unknown> | null) { super(message); }
}

/** Called when the server says the sign-in is no longer valid (set by main.ts: back to the sign-in page). */
export let onSignedOut = () => {};
export function setSignedOutHandler(f: () => void) { onSignedOut = f; }

export async function api<T = unknown>(method: string, path: string, body?: unknown): Promise<T> {
  const headers: Record<string, string> = { 'X-Token': session.token, Accept: 'application/json' };
  const opt: RequestInit = { method, headers };
  if (body !== undefined) { headers['Content-Type'] = 'application/json'; opt.body = JSON.stringify(body); }
  let r: Response;
  try { r = await fetch(path, opt); } catch {
    throw new ApiError(t('No connection to the server. Check the network and try again.'), 0, null);
  }
  let j: Record<string, unknown> | null = null;
  try { j = await r.json(); } catch { /* not JSON */ }
  if (r.status === 401 && session.token && !path.startsWith('/api/login')) {
    forgetToken(); onSignedOut();
    throw new ApiError(t('Please sign in again'), 401, j);
  }
  if (!r.ok) {
    const msg = j && typeof j.error === 'string' ? t(j.error) : t('The server answered with an error ({n}).', { n: r.status });
    throw new ApiError(msg, r.status, j);
  }
  return j as T;
}
export const get = <T = unknown>(path: string) => api<T>('GET', path);

/** The list in a MediaContainer answer (Metadata, Hub, Directory …). */
export function list<T>(j: unknown, key = 'Metadata'): T[] {
  const mc = j && typeof j === 'object' ? (j as { MediaContainer?: Record<string, unknown> }).MediaContainer : undefined;
  const v = mc && mc[key];
  return Array.isArray(v) ? (v as T[]) : [];
}

// ---------- tickets
// Picture, video and subtitle addresses carry a short-lived ticket (GET /api/ticket) instead of the sign-in
// token, which would otherwise end up in proxy logs and the browser history. '' = pictures, 'item=<id>' = one
// item's video and subtitles.
const tickets = new Map<string, { ticket: string, expires: number }>();
/** The picture ticket; pictures are drawn again when it is renewed. */
export const pictureTicket = ref('');

export async function ticket(q = ''): Promise<string> {
  const c = tickets.get(q);
  if (c && c.expires - Date.now() / 1000 > 600) return c.ticket;
  const r = await api<{ ticket: string, expires: number }>('GET', '/api/ticket' + (q ? '?' + q : ''));
  tickets.set(q, r);
  if (!q) pictureTicket.value = r.ticket;
  return r.ticket;
}
setInterval(() => { if (session.me) ticket().catch(() => {}); }, 5 * 60 * 1000);
document.addEventListener('visibilitychange', () => { if (!document.hidden && session.me) ticket().catch(() => {}); });

export const withTicket = (p: string, tk: string) => p + (p.includes('?') ? '&' : '?') + 'ticket=' + encodeURIComponent(tk);
/** A picture address (scaled down on the server to about w px wide when given). */
export function picture(p: string | undefined | null, w?: number): string {
  if (!p) return '';
  if (/^https?:/.test(p)) return p;
  return withTicket(w ? p + (p.includes('?') ? '&' : '?') + 'w=' + w : p, pictureTicket.value);
}

// ---------- start
/** Asks who is signed in and what they may see. Without a valid sign-in, session.me stays null. */
export async function boot() {
  const [status, me, secs] = await Promise.all([
    get<Status>('/api/status').catch(() => null),
    session.token ? get<Me>('/api/me').catch(() => null) : Promise.resolve(null),
    session.token ? get<Container<'Directory', Section>>('/library/sections').catch(() => null) : Promise.resolve(null),
  ]);
  session.status = status;
  if (me && secs) {
    await ticket().catch(() => {});
    session.me = me;
    session.sections = list<Section>(secs, 'Directory');
  }
}

export async function signIn(token: string) {
  saveToken(token);
  await boot();
}

export async function signOut() {
  try { await api('POST', '/api/logout'); } catch { /* signed out anyway */ }
  forgetToken();
}

/** Settings that follow the person from browser to browser (POST /api/me/prefs); they count here at once. */
export function savePref(key: string, value: string | null) {
  const me = session.me;
  if (!me) return Promise.resolve();
  const prefs = { ...(me.prefs || {}) };
  if (value === null) delete prefs[key]; else prefs[key] = value;
  me.prefs = prefs;
  return api('POST', '/api/me/prefs', { [key]: value }).then(() => {});
}
/** One of those settings, or def when it was never set. */
export const pref = (key: string, def = '') => (session.me && session.me.prefs && key in session.me.prefs ? session.me.prefs[key] : def);
