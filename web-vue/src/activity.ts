// What the server is doing in the background (GET /api/metadata/status), for the owner: scans, a job such as
// credits detection or an import, details being downloaded. Shown at the top (App.vue); looked at every 3
// seconds while something runs, else every 20 seconds. poke() looks at once (after starting something).
import { reactive } from 'vue';
import { api, get, session } from './api';
import { fmtNum } from './format';
import { t } from './i18n';
import { toast } from './toast';

interface ScanProgress { name: string, phase?: string, done?: number, total?: number, current?: string }
interface Status {
  queued: number, done: number, working_on?: number | null, working_title?: string, scanning?: string[], scan_progress?: ScanProgress[],
  job?: { label: string, done: number, total: number } | null, last?: { message?: string, ok?: boolean } | null, playing?: number,
}
export interface Line { text: string, pct?: number, title?: string }

export const activity = reactive({
  busy: false,
  lines: [] as Line[],
  /** The last thing finished, shown for a few seconds. */
  last: '',
  canCancel: false,
  /** How many play right now (the dashboard button at the top); null before the first answer. */
  playing: null as number | null,
  /** Counts up when something finished (pages that show counts read them again). */
  finished: 0,
});

let timer = 0;
let done = -1;
let wasBusy = false;
let lastTimer = 0;

const scanNote = (p: ScanProgress) => p.phase === 'reading' && p.total ? t('Scanning {name}: {done} of {total}', { name: p.name, done: fmtNum(p.done || 0), total: fmtNum(p.total) })
  : p.phase === 'finishing' ? t('Scanning {name}: finishing', { name: p.name })
  : p.done ? t('Scanning {name}: looking through the folders, {n} videos found', { name: p.name, n: fmtNum(p.done) })
  : t('Scanning {name}: looking through the folders', { name: p.name });

export async function poke() {
  clearTimeout(timer);
  if (!session.me || !session.me.admin) { activity.busy = false; activity.lines = []; return; }
  let st: Status;
  try { st = await get<Status>('/api/metadata/status'); } catch { timer = window.setTimeout(poke, 10000); return; }
  const lines: Line[] = [];
  const scans = st.scan_progress || [];
  for (const name of (st.scanning || []).filter(n => !scans.some(p => p.name === n))) lines.push({ text: t('Scanning {name}', { name }) });
  for (const p of scans) lines.push({ text: scanNote(p), pct: p.phase === 'reading' && p.total ? Math.round(100 * (p.done || 0) / p.total) : undefined, title: p.current });
  if (st.job) {
    const pct = Math.round(100 * st.job.done / Math.max(1, st.job.total));
    // Optimizing counts in hundredths of a video: shown as a percentage.
    lines.push({ text: st.job.label === 'Optimizing' ? t('{label}: {pct} %', { label: t(st.job.label), pct: Math.floor(pct) })
      : t('{label}: {done} of {total}', { label: t(st.job.label), done: st.job.done, total: st.job.total }), pct });
  }
  if (st.working_on || st.queued > 0) lines.push({ text: t('Updating details') + (st.working_title ? ': ' + st.working_title : '') + (st.queued > 0 ? ' · ' + t('{n} left', { n: st.queued }) : '') });
  activity.lines = lines;
  if (typeof st.playing === 'number') activity.playing = st.playing;
  activity.busy = lines.length > 0;
  activity.canCancel = !!st.job || st.queued > 0;
  if ((done >= 0 && st.done > done) || (wasBusy && !activity.busy)) {
    activity.finished++;
    if (!activity.busy && st.last && st.last.message) {
      activity.last = (st.last.ok === false ? '⚠ ' : '✓ ') + t(st.last.message);
      clearTimeout(lastTimer);
      lastTimer = window.setTimeout(() => { activity.last = ''; }, 8000);
    }
  }
  if (activity.busy) activity.last = '';
  done = st.done; wasBusy = activity.busy;
  timer = window.setTimeout(poke, activity.busy ? 3000 : 20000);
}

/** Stops what goes file by file (after the file it is on); waiting details are dropped. */
export async function cancelJob() {
  try { await api('POST', '/api/job/cancel'); toast(t('Stopping after the current file…')); } catch (e) { toast((e as Error).message, 6000); }
  poke();
}
