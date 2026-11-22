// Downloads: the qualities a copy can have, saving a ready file, and how many still convert (the number next
// to "Downloads" in the left menu).
import { reactive } from 'vue';
import { get, ticket, withTicket, session } from './api';
import { fmtNum } from './format';
import { t } from './i18n';

// As the player's, without the two biggest 1080p ones (a download that big might as well be the original).
export const DL_QUALITIES: [string, string][] = [['original', 'Original quality'], ['1080p-8', '1080p 8 Mbps'], ['720p-4', '720p 4 Mbps'],
  ['720p-3', '720p 3 Mbps'], ['720p-2', '720p 2 Mbps'], ['480p-1.5', '480p 1.5 Mbps'], ['320p-0.7', '320p 0.7 Mbps']];
export const dlQualityName = (q: string) => { const x = DL_QUALITIES.find(y => y[0] === q); return !x ? q : q === 'original' ? t(x[1]) : x[1].replace(/\d+\.\d+/, n => fmtNum(Number(n))); };
// About how big a converted copy gets: picture plus 160 kbit/s stereo sound.
export const dlEstimate = (q: string, ms: number) => { const m = /^(\d+)p-([\d.]+)$/.exec(q); return m && ms ? (Number(m[2]) * 1000 + 160) * 125 * ms / 1000 : 0; };

/** Saves a ready file with a hidden link: the page stays. Its ticket opens only this file. */
export async function saveFile(url: string) {
  const tk = await ticket('path=' + encodeURIComponent(url.split('?')[0]));
  const a = document.createElement('a');
  a.href = withTicket(url, tk); a.download = ''; a.style.display = 'none';
  document.body.append(a); a.click(); a.remove();
}

export interface DownloadList {
  downloads: Download[], allowed: boolean, keep_days?: number, queued: number, converting: number, ready: number, failed: number,
}
export interface Download {
  id: number, state: string, quality: string, progress: number, size?: number, parts: number, url: string, thumb?: string, user?: string,
  original?: boolean, error?: string, expires_at?: number,
  item?: { id: number, type: string, title?: string, grandparentTitle?: string, parentIndex?: number, index?: number },
}
export const dl = reactive({ busy: 0 });
export async function loadDlCount(list?: DownloadList) {
  if (!session.me || !session.me.can_download) { dl.busy = 0; return; }
  try { list = list || await get<DownloadList>('/api/downloads'); } catch { return; }
  dl.busy = list.queued + list.converting;
}
