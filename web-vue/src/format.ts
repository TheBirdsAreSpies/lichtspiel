// Numbers, times and names as they are shown.
import { lang, t } from './i18n';
import { pref, type Item, type Playlist } from './api';

export function fmtDur(ms?: number): string {
  if (!ms) return '';
  const m = Math.round(ms / 60000);
  if (m < 1) return Math.max(1, Math.round(ms / 1000)) + ' s';
  return m >= 60 ? t('{h} h {m} min', { h: Math.floor(m / 60), m: m % 60 }) : t('{m} min', { m });
}

/** 1:02:03 or 2:03. */
export function fmtTime(s: number): string {
  s = Math.max(0, Math.floor(s));
  const hh = Math.floor(s / 3600), mm = Math.floor(s / 60) % 60, ss = s % 60;
  return (hh ? hh + ':' + String(mm).padStart(2, '0') : mm) + ':' + String(ss).padStart(2, '0');
}

/** A number as the language writes it; `digits` fixes the places after the point. */
export const fmtNum = (n: number, digits?: number) => Number(n).toLocaleString(lang.value, digits === undefined ? undefined : { minimumFractionDigits: digits, maximumFractionDigits: digits });

/** A time of day (14:05, or 2:05 PM), as Settings → Time format says. */
export function fmtClock(d: Date): string {
  const f = pref('timeFormat', 'auto');
  return d.toLocaleTimeString(lang.value, { hour: '2-digit', minute: '2-digit', hour12: f === '12' ? true : f === '24' ? false : undefined });
}

/** Where to go on from, in seconds; less than a second in counts as not started. */
/** A file size: 1,4 GB, 700 MB. */
export const fmtSize = (b: number) => (b > 1e9 ? fmtNum(b / 1e9, 1) + ' GB' : fmtNum(b / 1e6, 0) + ' MB');
export const resumeAt = (it: Item) => ((it.viewOffset || 0) >= 1000 ? (it.viewOffset || 0) / 1000 : 0);

export const seasonTitle = (s?: string) => (/^(Season \d+|Specials)$/.test(s || '') ? t(s || '') : s || '');

export const isPlayable = (it: Item) => ['movie', 'episode', 'video', 'clip', 'track'].includes(it.type);
/** Artists, albums and songs: square covers. */
export const isMusic = (it: Item) => ['artist', 'album', 'track'].includes(it.type);

/** The kinds of an album, in the order an artist's page lists them. */
export const RELEASES = ['album', 'ep', 'single', 'live', 'compilation', 'soundtrack'];
export const releaseName = (r: string, plural = false) => ({ album: plural ? t('Albums') : t('Album'), ep: plural ? t('EPs') : t('EP'), single: plural ? t('Singles') : t('Single'),
  live: plural ? t('Live albums') : t('Live'), compilation: plural ? t('Compilations') : t('Compilation'), soundtrack: plural ? t('Soundtracks') : t('Soundtrack') } as Record<string, string>)[r] || r;

/** Whether a poster shows the watched mark: watched, and of a kind chosen under Settings → Playback. */
export const showSeen = (it: Item) => {
  const mark = pref('watchedMark', 'all');
  return mark !== 'none' && (mark === 'all' || (mark === 'movies') === (it.type === 'movie')) && isWatched(it);
};
export const isWatched = (it: Item) => (it.leafCount ? (it.viewedLeafCount || 0) >= it.leafCount : (it.viewCount || 0) > 0);

const L3: Record<string, string> = { ger: 'de', deu: 'de', eng: 'en', fre: 'fr', fra: 'fr', spa: 'es', ita: 'it', jpn: 'ja', por: 'pt', rus: 'ru',
  dut: 'nl', nld: 'nl', pol: 'pl', tur: 'tr', swe: 'sv', dan: 'da', nor: 'no', fin: 'fi', kor: 'ko', chi: 'zh', zho: 'zh' };
/** "ger", "deu", "de-DE" → "de". */
export const lang2 = (c?: string) => { const s = String(c || '').toLowerCase().split(/[-_]/)[0]; return s.length === 3 ? (L3[s] || s) : s; };

export function langName(code?: string): string {
  if (!code) return '';
  try { return new Intl.DisplayNames([lang.value], { type: 'language' }).of(lang2(code)) || code; } catch { return code; }
}

/** The line under a poster's title. */
export function cardLines(it: Item): [string, string] {
  if (it.type === 'episode') return [it.grandparentTitle || it.title, `${t('S{n}', { n: it.parentIndex || 0 })} ${t('E{n}', { n: it.index || 0 })} · ${it.title}`];
  if (it.type === 'season') return [seasonTitle(it.title), it.parentTitle || ''];
  if (it.type === 'show') return [it.title, t('{n} episodes', { n: it.leafCount || 0 })];
  if (it.type === 'artist') return [it.title, t('{n} songs', { n: it.leafCount || 0 })];
  if (it.type === 'album') return [it.title, [it.parentTitle, it.year, it.releaseType && it.releaseType !== 'album' ? releaseName(it.releaseType) : ''].filter(Boolean).join(' · ')];
  if (it.type === 'track') return [it.title, [it.grandparentTitle, it.parentTitle].filter(Boolean).join(' · ')];
  return [it.title, [it.year, it.editionTitle].filter(Boolean).join(' · ')];
}

/** A playlist's kind as shown: normal, smart, default. */
export const kindName = (k: string) => ({ manual: t('normal'), smart: t('smart'), default: t('default') }[k] || k);
/** The server's default playlists have English names, shown in the chosen language. */
export const plTitle = (pl: Playlist) => (pl.default ? t(pl.title) : pl.title);

// ---- numbers of the dashboard
export const hour12 = () => { const f = pref('timeFormat', 'auto'); return f === '12' ? true : f === '24' ? false : undefined; };
/** A date and time as the language writes it (unix seconds). */
export const fmtDateTime = (sec?: number) => (sec ? new Date(sec * 1000).toLocaleString(lang.value, { hour12: hour12() }) : '');
export const fmtDate = (sec?: number) => (sec ? new Date(sec * 1000).toLocaleDateString(lang.value) : '');
/** Bytes per second as bits: 12,5 Mbit/s. */
export const fmtBits = (bps: number) => { const b = bps * 8; return b >= 1e9 ? fmtNum(b / 1e9, 2) + ' Gbit/s' : b >= 1e6 ? fmtNum(b / 1e6, 1) + ' Mbit/s' : b >= 1e3 ? fmtNum(b / 1e3, 0) + ' kbit/s' : fmtNum(b, 0) + ' bit/s'; };
export const fmtBytes = (b: number) => (b >= 1e12 ? fmtNum(b / 1e12, 2) + ' TB' : b >= 1e9 ? fmtNum(b / 1e9, 1) + ' GB' : b >= 1e6 ? fmtNum(b / 1e6, 0) + ' MB' : fmtNum(b / 1e3, 0) + ' kB');
export const fmtPct = (v?: number | null) => (v === null || v === undefined ? '–' : fmtNum(v, v < 10 ? 1 : 0) + ' %');
/** "just now", "5 min ago", "3 h ago", else the date. */
export function fmtAgo(sec: number, now: number): string {
  const d = Math.max(0, now - sec);
  if (d < 60) return t('just now');
  if (d < 3600) return t('{n} min ago', { n: Math.floor(d / 60) });
  if (d < 86400) return t('{n} h ago', { n: Math.floor(d / 3600) });
  return fmtDate(sec);
}
export const fmtUptime = (s: number) => { const d = Math.floor(s / 86400), h = Math.floor(s / 3600) % 24, m = Math.floor(s / 60) % 60; return d ? t('{d} d {h} h', { d, h }) : h ? t('{h} h {m} min', { h, m }) : t('{m} min', { m }); };
/** Round steps for a chart's axis: four even steps of a round size up to at least v (0, 10, 20, 30, 40 for 33.4). */
const niceMax = (v: number) => { if (!(v > 0)) return 1; const p = Math.pow(10, Math.floor(Math.log10(v))); const n = v / p; return (n <= 1 ? 1 : n <= 2 ? 2 : n <= 2.5 ? 2.5 : n <= 5 ? 5 : 10) * p; };
export const niceTop = (v: number) => 4 * niceMax(Math.max(v, 1e-9) / 4);
