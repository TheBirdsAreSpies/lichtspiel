// Sending the video to a Chromecast. Google's Cast library only works in Chrome and Edge, and only on an
// https:// address or localhost; it is loaded once, the first time the player opens. It plays on Google's
// Default Media Receiver: the TV fetches the same addresses the browser would (the file as it is, or the
// converted MP4 stream from the seek point on, each with the item's ticket), and the player in the browser
// becomes its remote control. Not on the TV: the speed, night mode and the volume levelling.
import { reactive } from 'vue';
import { session } from './api';

/* eslint-disable @typescript-eslint/no-explicit-any -- Google's library has no types here */
declare global { interface Window { __onGCastApiAvailable?: (ok: boolean) => void, cast?: any, chrome?: any } }

/** What the player shows: whether a TV can be picked, whether one plays, and what it says. */
export const cast = reactive({ available: false, on: false, device: '', paused: true, time: 0, volume: 1, muted: false, buffering: false });
/** What the player does when a TV takes over, lets go, or comes to the end. */
export const castHooks: { started?: () => void, ended?: () => void, finished?: () => void, failed?: () => void } = {};

let ctx: any = null, rp: any = null, rpc: any = null, media: any = null, tried = false;

export function initCast() {
  if (tried) return;
  tried = true;
  const ua = navigator.userAgent;
  if (!window.isSecureContext || !window.chrome || !/Chrome\//.test(ua) || /OPR\/|SamsungBrowser|Silk/.test(ua)) return;
  window.__onGCastApiAvailable = ok => { if (ok && window.cast && window.cast.framework) setup(); };
  const s = document.createElement('script');
  s.src = 'https://www.gstatic.com/cv/js/sender/v1/cast_sender.js?loadCastFramework=1';
  s.async = true;
  document.head.append(s);
}

function setup() {
  const F = window.cast.framework, chrome = window.chrome;
  ctx = F.CastContext.getInstance();
  ctx.setOptions({ receiverApplicationId: chrome.cast.media.DEFAULT_MEDIA_RECEIVER_APP_ID, autoJoinPolicy: chrome.cast.AutoJoinPolicy.ORIGIN_SCOPED });
  rp = new F.RemotePlayer();
  rpc = new F.RemotePlayerController(rp);
  const state = (s: string) => { cast.available = s !== F.CastState.NO_DEVICES_AVAILABLE; };
  state(ctx.getCastState());
  ctx.addEventListener(F.CastContextEventType.CAST_STATE_CHANGED, (e: any) => state(e.castState));
  ctx.addEventListener(F.CastContextEventType.SESSION_STATE_CHANGED, (e: any) => {
    if (e.sessionState === F.SessionState.SESSION_STARTED || e.sessionState === F.SessionState.SESSION_RESUMED) started();
    else if (e.sessionState === F.SessionState.SESSION_ENDED) ended();
  });
  const E = F.RemotePlayerEventType;
  rpc.addEventListener(E.CURRENT_TIME_CHANGED, () => { cast.time = rp.currentTime || 0; });
  rpc.addEventListener(E.IS_PAUSED_CHANGED, () => { if (media) cast.paused = !!rp.isPaused; });
  rpc.addEventListener(E.VOLUME_LEVEL_CHANGED, () => { cast.volume = rp.volumeLevel; });
  rpc.addEventListener(E.IS_MUTED_CHANGED, () => { cast.muted = !!rp.isMuted; });
  rpc.addEventListener(E.PLAYER_STATE_CHANGED, () => { cast.buffering = rp.playerState === chrome.cast.media.PlayerState.BUFFERING; });
}

function started() {
  const s = ctx.getCurrentSession();
  if (!s) return;
  cast.on = true;
  cast.device = (s.getCastDevice() && s.getCastDevice().friendlyName) || 'Chromecast';
  cast.volume = rp.volumeLevel ?? 1; cast.muted = !!rp.isMuted;
  if (castHooks.started) castHooks.started();
}
function ended() {
  if (!cast.on) return;
  cast.on = false; media = null; cast.buffering = false;
  if (castHooks.ended) castHooks.ended();
}

/** Chrome's own list of devices; while casting it also has "Stop casting". */
export const castPick = () => { if (ctx) ctx.requestSession().catch(() => {}); };
/** Stops the TV (when the player is closed). */
export function castStop() { if (!cast.on || !ctx) return; media = null; ctx.endCurrentSession(true); }

/** The address the TV reaches the server at: this page's, unless it was opened as localhost. */
export function castUrl(path: string) {
  const local = /^(localhost|127\.|\[::1\])/.test(location.hostname);
  const lan = session.status && session.status.lan_url;
  return (local && lan ? lan : location.origin).replace(/\/$/, '') + path;
}

export interface CastMedia {
  url: string, type: string, title: string, subtitle: string, image?: string, at: number,
  sub?: { url: string, name: string, language: string, scale: number, color: string, background: boolean },
}
/** Plays an address on the TV from `at` (s, in that file). False when the TV could not. */
export async function castLoad(m: CastMedia): Promise<boolean> {
  const s = ctx && ctx.getCurrentSession();
  if (!s) return false;
  const C = window.chrome.cast;
  const mi = new C.media.MediaInfo(m.url, m.type);
  mi.streamType = C.media.StreamType.BUFFERED;
  const md = new C.media.GenericMediaMetadata();
  md.title = m.title; md.subtitle = m.subtitle;
  if (m.image) md.images = [new C.Image(m.image)];
  mi.metadata = md;
  if (m.sub) {
    const tr = new C.media.Track(1, C.media.TrackType.TEXT);
    tr.trackContentId = m.sub.url; tr.trackContentType = 'text/vtt';
    tr.subtype = C.media.TextTrackType.SUBTITLES; tr.name = m.sub.name; tr.language = m.sub.language;
    mi.tracks = [tr];
    const style = new C.media.TextTrackStyle();
    style.fontScale = m.sub.scale;
    style.foregroundColor = (m.sub.color + 'FF').toUpperCase();
    style.backgroundColor = m.sub.background ? '#0000008C' : '#00000000';
    mi.textTrackStyle = style;
  }
  const req = new C.media.LoadRequest(mi);
  req.currentTime = m.at; req.autoplay = true;
  if (m.sub) req.activeTrackIds = [1];
  media = null; cast.buffering = true;
  try { await s.loadMedia(req); } catch { cast.buffering = false; return false; }
  const ms = s.getMediaSession();
  media = ms; cast.buffering = false; cast.paused = false;
  if (ms) ms.addUpdateListener(() => {
    if (media !== ms || ms.playerState !== C.media.PlayerState.IDLE) return;
    media = null;
    if (ms.idleReason === C.media.IdleReason.FINISHED) { if (castHooks.finished) castHooks.finished(); }
    else if (ms.idleReason === C.media.IdleReason.ERROR && castHooks.failed) castHooks.failed();
  });
  return true;
}
export function castSeek(s: number) { if (!rp) return; rp.currentTime = s; rpc.seek(); }
export function castToggle() { if (media) rpc.playOrPause(); }
export function castPause() { if (media && !rp.isPaused) rpc.playOrPause(); }
export function castVolume(v: number) {
  if (!rp) return;
  if (v > 0 && rp.isMuted) rpc.muteOrUnmute();
  rp.volumeLevel = v; rpc.setVolumeLevel();
}
export function castMute() { if (rpc) rpc.muteOrUnmute(); }
