// Numbers, times and names as they are shown.
import { lang, t } from './i18n';
import type { Item, Playlist } from './api';

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

export const fmtNum = (n: number) => Number(n).toLocaleString(lang.value);

/** Where to go on from, in seconds; less than a second in counts as not started. */
export const resumeAt = (it: Item) => ((it.viewOffset || 0) >= 1000 ? (it.viewOffset || 0) / 1000 : 0);

export const seasonTitle = (s?: string) => (/^(Season \d+|Specials)$/.test(s || '') ? t(s || '') : s || '');

export const isPlayable = (it: Item) => ['movie', 'episode', 'clip'].includes(it.type);

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
  return [it.title, [it.year, it.editionTitle].filter(Boolean).join(' · ')];
}

/** A playlist's kind as shown: normal, smart, default. */
export const kindName = (k: string) => ({ manual: t('normal'), smart: t('smart'), default: t('default') }[k] || k);
/** The server's default playlists have English names, shown in the chosen language. */
export const plTitle = (pl: Playlist) => (pl.default ? t(pl.title) : pl.title);
