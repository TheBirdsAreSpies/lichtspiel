// Lichtspiel web app: sending the video to a Chromecast. Loaded after player.js.
'use strict';

// Google's Cast library only works in Chrome and Edge, and only on an https:// address or localhost.
// It plays on Google's Default Media Receiver: the TV fetches the same addresses the browser would
// (the file as it is, or the converted MP4 stream from the seek point on, each with the item's ticket),
// and the player in the browser becomes its remote control: play / pause, the seek bar, ±10 / 30 s,
// volume, subtitles, audio, quality, skip intro / credits and the next episode work as before.
// Not on the TV: the speed, night mode and the volume levelling (they are done in the browser).
const CAST = { on: false, device: '', ctx: null, rp: null, rpc: null, media: null };
const castOn = () => CAST.on;
const isPaused = () => (CAST.on ? !!(CAST.rp && CAST.rp.isPaused) : video.paused);
const CAST_ICON = 'M1 18v3h3c0-1.66-1.34-3-3-3zm0-4v2c2.76 0 5 2.24 5 5h2c0-3.87-3.13-7-7-7zm0-4v2a9 9 0 0 1 9 9h2c0-6.08-4.93-11-11-11zm20-7H3c-1.1 0-2 .9-2 2v3h2V5h18v14h-7v2h7c1.1 0 2-.9 2-2V5c0-1.1-.9-2-2-2z';
ICONS.cast = CAST_ICON;
btn('pcast', 'cast', t('Play on a TV (Chromecast)'));
$('#pcast').hidden = true;

(function castInit() {
  const ua = navigator.userAgent;
  if (!window.isSecureContext || !window.chrome || !/Chrome\//.test(ua) || /OPR\/|SamsungBrowser|Silk/.test(ua)) return;
  window.__onGCastApiAvailable = ok => { if (ok) castSetup(); };
  const s = document.createElement('script');
  s.src = 'https://www.gstatic.com/cv/js/sender/v1/cast_sender.js?loadCastFramework=1';
  s.async = true;
  document.head.append(s);
})();

function castSetup() {
  const F = cast.framework;
  CAST.ctx = F.CastContext.getInstance();
  CAST.ctx.setOptions({ receiverApplicationId: chrome.cast.media.DEFAULT_MEDIA_RECEIVER_APP_ID, autoJoinPolicy: chrome.cast.AutoJoinPolicy.ORIGIN_SCOPED });
  CAST.rp = new F.RemotePlayer();
  CAST.rpc = new F.RemotePlayerController(CAST.rp);
  const showButton = state => {
    $('#pcast').hidden = state === F.CastState.NO_DEVICES_AVAILABLE;
    $('#pcast').classList.toggle('on', state === F.CastState.CONNECTED);
  };
  showButton(CAST.ctx.getCastState());
  CAST.ctx.addEventListener(F.CastContextEventType.CAST_STATE_CHANGED, e => showButton(e.castState));
  CAST.ctx.addEventListener(F.CastContextEventType.SESSION_STATE_CHANGED, e => {
    if (e.sessionState === F.SessionState.SESSION_STARTED || e.sessionState === F.SessionState.SESSION_RESUMED) castStarted();
    else if (e.sessionState === F.SessionState.SESSION_ENDED) castEnded();
  });
  const E = F.RemotePlayerEventType;
  CAST.rpc.addEventListener(E.CURRENT_TIME_CHANGED, () => { if (CAST.on && P.it && !bar.classList.contains('drag')) drawBar(); });
  CAST.rpc.addEventListener(E.IS_PAUSED_CHANGED, () => {
    if (!CAST.on || !P.it || !CAST.media) return;
    drawButtons(); wake();
    report(CAST.rp.isPaused ? 'paused' : 'playing');
  });
  CAST.rpc.addEventListener(E.VOLUME_LEVEL_CHANGED, () => { if (CAST.on) drawButtons(); });
  CAST.rpc.addEventListener(E.IS_MUTED_CHANGED, () => { if (CAST.on) drawButtons(); });
  CAST.rpc.addEventListener(E.PLAYER_STATE_CHANGED, () => {
    if (!CAST.on) return;
    const st = CAST.rp.playerState;
    spin(st === chrome.cast.media.PlayerState.BUFFERING);
  });
}

$('#pcast').addEventListener('click', () => {
  // Chrome's own list of devices; while casting it also has "Stop casting".
  if (CAST.ctx) CAST.ctx.requestSession().catch(() => {});
});

function castStarted() {
  const s = CAST.ctx.getCurrentSession();
  if (!s) return;
  const at = P.it ? position() : 0;
  CAST.on = true;
  CAST.device = (s.getCastDevice() && s.getCastDevice().friendlyName) || 'Chromecast';
  $('#pcastinfo').replaceChildren(icon('cast'), h('div', {}, t('Playing on {device}', { device: CAST.device })));
  player.classList.add('casting');
  if (!P.it) return;
  // The browser stops; the TV goes on from the same place.
  video.pause(); video.removeAttribute('src'); video.querySelectorAll('track').forEach(tr => tr.remove()); video.load();
  load(at);
}

function castEnded() {
  if (!CAST.on) return;
  const at = P.it ? position() : 0;
  CAST.on = false; CAST.media = null;
  player.classList.remove('casting');
  spin(false);
  // Back in the browser, from where the TV was.
  if (P.it && player.style.display === 'flex') load(at);
}

// Stops the TV when the player is closed.
function castStop() {
  if (!CAST.on || !CAST.ctx) return;
  CAST.media = null;
  CAST.ctx.endCurrentSession(true);
}

// The address the TV reaches the server at: this page's, unless it was opened as localhost.
function castUrl(path) {
  const local = /^(localhost|127\.|\[::1\])/.test(location.hostname);
  const base = local && S.status && S.status.lan_url ? S.status.lan_url : location.origin;
  return base.replace(/\/$/, '') + path;
}

// Called by load() instead of setting the <video>'s address. direct: the file as it is (the TV seeks in it);
// else a converted stream that starts at `at` (seeking loads it again, like in the browser).
async function castLoad(it, at, q, extra) {
  const s = CAST.ctx && CAST.ctx.getCurrentSession();
  if (!s) return;
  const direct = P.method === 'direct';
  P.offset = direct ? 0 : at;
  const dev = '&X-Plex-Product=Chromecast&X-Plex-Device-Name=' + encodeURIComponent(CAST.device);
  const path = direct ? `/library/parts/${it.id}/file${P.part ? '?part=' + P.part : ''}`
    : `/video/transcode/${it.id}/stream.mp4?quality=${q}&offset=${at.toFixed(1)}${extra}${store.get('directPlay', '1') === '0' ? '&transcode=1' : ''}`;
  const container = String((it.Media && it.Media[0] && it.Media[0].container) || '').toLowerCase();
  const mi = new chrome.cast.media.MediaInfo(castUrl(withTicket(path, P.tk)) + dev, direct && container === 'webm' ? 'video/webm' : 'video/mp4');
  mi.streamType = chrome.cast.media.StreamType.BUFFERED;
  const md = new chrome.cast.media.GenericMediaMetadata();
  md.title = it.type === 'episode' ? `${it.grandparentTitle} · ${nextName(it)}` : it.title;
  md.subtitle = P.subLine || '';
  const thumb = it.type === 'episode' && it.grandparentThumb ? it.grandparentThumb : it.thumb;
  if (thumb) md.images = [new chrome.cast.Image(castUrl(withTok(thumb)))];
  mi.metadata = md;
  // A text subtitle goes along (only with the first file of a split movie, as in the browser).
  const sub = P.sub >= 0 && !P.part ? P.subs[P.sub] : null;
  if (sub) {
    const tr = new chrome.cast.media.Track(1, chrome.cast.media.TrackType.TEXT);
    tr.trackContentId = castUrl(withTicket(sub.key + (direct ? '' : '?from=' + at.toFixed(1)), P.tk));
    tr.trackContentType = 'text/vtt';
    tr.subtype = chrome.cast.media.TextTrackType.SUBTITLES;
    tr.name = subName(sub, P.sub);
    tr.language = sub.language || 'und';
    mi.tracks = [tr];
    const style = new chrome.cast.media.TextTrackStyle();
    style.fontScale = (Number(P.subSize) || 100) / 100;
    style.foregroundColor = (store.get('subColor', '#ffffff') + 'FF').toUpperCase();
    style.backgroundColor = store.get('subBg', '1') === '1' ? '#0000008C' : '#00000000';
    mi.textTrackStyle = style;
  }
  const req = new chrome.cast.media.LoadRequest(mi);
  req.currentTime = direct ? at : 0;
  req.autoplay = true;
  if (sub) req.activeTrackIds = [1];
  $('#pmethod').textContent = ' · ' + t('on {device}', { device: CAST.device });
  spin(true);
  CAST.media = null;
  try {
    await s.loadMedia(req);
  } catch (e) {
    spin(false);
    toast(t('The TV could not play this video.') + (e && e.code ? ' (' + e.code + ')' : ''), 6000);
    return;
  }
  const m = s.getMediaSession();
  CAST.media = m;
  spin(false);
  report('playing');
  drawButtons(); drawBar();
  if (m) m.addUpdateListener(() => {
    if (CAST.media !== m || m.playerState !== chrome.cast.media.PlayerState.IDLE) return;
    CAST.media = null;
    if (m.idleReason === chrome.cast.media.IdleReason.FINISHED) videoEnded();
    else if (m.idleReason === chrome.cast.media.IdleReason.ERROR) toast(t('The TV could not play this video.'), 6000);
  });
}

function castSeek(s) {
  CAST.rp.currentTime = s;
  CAST.rpc.seek();
}
function castTogglePlay() {
  if (!CAST.media) return;
  flash(CAST.rp.isPaused ? 'play' : 'pause');
  CAST.rpc.playOrPause();
}
function castVolume(v) {
  if (v > 0 && CAST.rp.isMuted) CAST.rpc.muteOrUnmute();
  CAST.rp.volumeLevel = v;
  CAST.rpc.setVolumeLevel();
}
