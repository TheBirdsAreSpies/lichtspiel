// Watch together: everyone in a room plays the same thing at the same place. Whoever plays, pauses, jumps
// or goes on to another episode moves everyone: the change goes to the server (POST /api/together/{code}),
// and the others ask for the room every 2 s and follow when its number (seq) went up. A change that came
// from the others is not sent back (for a moment, `quiet`).
import { reactive } from 'vue';
import { api, get } from './api';

export interface Room { item: number, position: number, paused: boolean, seq: number, by?: string, members: { id: number, name: string }[] }
export interface Invite { code: string, by: string, title: string, members: string[] }

export const tg = reactive({ code: '', seq: 0, quiet: 0, members: [] as Room['members'] });

/** Into a room: what it plays and where, for the player to start with. */
export async function joinRoom(code: string): Promise<Room> {
  const st = await get<Room>('/api/together/' + code);
  tg.code = code; tg.seq = st.seq; tg.quiet = Date.now() + 3000; tg.members = st.members;
  return st;
}
/** Something changed here: tell the others (not while following a change of theirs). */
export function tgSend(position: number, paused: boolean, extra: Record<string, unknown> = {}) {
  if (!tg.code || Date.now() < tg.quiet) return;
  api<{ seq: number }>('POST', '/api/together/' + tg.code, { position: Math.round(position * 1000), paused, ...extra })
    .then(st => { tg.seq = Math.max(tg.seq, st.seq); }).catch(() => {});
}
export function tgLeave() {
  if (tg.code) api('DELETE', '/api/together/' + tg.code).catch(() => {});
  tg.code = ''; tg.members = [];
}

// ---- invitations from others here: looked for every 15 seconds (App.vue), each shown once
export const invites = reactive({ shown: null as Invite | null });
const seen = new Set<string>();
export async function checkInvites() {
  if (tg.code || document.hidden || invites.shown) return;
  let r: { invites: Invite[] };
  try { r = await get('/api/together/invites'); } catch { return; }
  const inv = r.invites.find(i => !seen.has(i.code));
  if (!inv) return;
  seen.add(inv.code);
  invites.shown = inv;
}
export function declineInvite(inv: Invite) { invites.shown = null; api('DELETE', `/api/together/${inv.code}/invite`).catch(() => {}); }
