<script setup lang="ts">
// The "⋯" of a title's page, as in the classic app: share, the owner's tools (details downloaded again,
// analyzing, the match, optimizing, the theme music, taking merged versions apart, who may see it), the play
// history, the media info and deleting the file. Each tool with its box.
import { computed, onBeforeUnmount, ref } from 'vue';
import { useRouter } from 'vue-router';
import { api, get, list, session, type Item } from '../api';
import { poke } from '../activity';
import { fmtDateTime, fmtDur, fmtNum, fmtSize, isPlayable, isWatched, seasonTitle } from '../format';
import AddToDialog from './AddToDialog.vue';
import { addToQueue } from '../queue';
import { t } from '../i18n';
import { toast } from '../toast';
import Icon from './Icon.vue';
import Modal from './Modal.vue';
import AccessDialog from './AccessDialog.vue';

// card: the menu of a poster (its small "⋯", the menu put on the page so the card does not cut it off),
// with what the title's page has as buttons first: select, watchlist, add to …, watched or not.
// cont: on the Continue Watching row (it can be taken off there).
const props = defineProps<{ item: Item, card?: boolean, selectable?: boolean, cont?: boolean }>();
const emit = defineEmits<{ changed: [], select: [], removed: [] }>();
const router = useRouter();
// (The page's item has more than the Item type says: the match, the optimized copy, every stream.)
type Any = Record<string, any>; // eslint-disable-line @typescript-eslint/no-explicit-any
const it = computed(() => props.item as Item & Any);
const x = computed<Any>(() => it.value.extra || {});
const admin = computed(() => !!(session.me && session.me.admin));
const matchable = computed(() => it.value.type === 'movie' || it.value.type === 'show');
const deletable = computed(() => !!(session.me && session.me.can_delete) && it.value.type !== 'clip' && !x.value.extra_type);
const name = computed(() => {
  const i = it.value;
  return i.type === 'episode' && i.grandparentTitle ? `${i.grandparentTitle} · ${i.title}` : i.type === 'season' ? `${i.parentTitle || ''} · ${seasonTitle(i.title)}` : i.title;
});

// ---- the menu itself
const open = ref(false);
const btn = ref<HTMLElement | null>(null);
const outside = (e: Event) => { if (btn.value && !btn.value.parentElement!.contains(e.target as Node)) open.value = false; };
const at = ref({ left: 0, top: 0 });
function toggle() {
  open.value = !open.value;
  if (open.value && props.card && btn.value) {
    // Below the button where there is room, else above it; kept inside the window.
    const b = btn.value.getBoundingClientRect(), w = 240, h = 420;
    at.value = { left: Math.max(8, Math.min(innerWidth - w - 8, b.right - w)), top: b.bottom + h < innerHeight ? b.bottom + 4 : Math.max(8, b.top - h) };
  }
  if (open.value) setTimeout(() => document.addEventListener('click', outside), 0); else document.removeEventListener('click', outside);
}
onBeforeUnmount(() => document.removeEventListener('click', outside));
type Box = '' | 'addto' | 'share' | 'match' | 'optimize' | 'access' | 'history' | 'info' | 'delete' | 'unmatch';
const box = ref<Box>('');
function pick(b: Box | (() => void)) {
  open.value = false; document.removeEventListener('click', outside);
  if (typeof b === 'function') b(); else { box.value = b; if (b === 'match') search(); if (b === 'history') loadHistory(); }
}
const fail = (e: unknown) => toast((e as Error).message, 6000);

// ---- a poster's own entries
async function watchlist() {
  try { await api('PUT', `/api/items/${it.value.id}/watchlist`, { watchlist: !it.value.watchlist }); toast(it.value.watchlist ? t('Removed from your watchlist') : t('Added to your watchlist')); emit('changed'); } catch (e) { fail(e); }
}
async function markWatched(watched: boolean) {
  try { await api('POST', `/:/${watched ? 'scrobble' : 'unscrobble'}?key=${it.value.id}`); toast(watched ? t('Marked as watched') : t('Marked as unwatched')); emit('changed'); } catch (e) { fail(e); }
}
async function uncontinue() {
  try { await api('POST', '/api/continue/remove', { ids: [it.value.id] }); toast(t('Removed from Continue Watching')); emit('removed'); } catch (e) { fail(e); }
}
const started = computed(() => !isWatched(it.value) && ((it.value.viewOffset || 0) > 0 || (it.value.viewedLeafCount || 0) > 0));

// ---- the owner's tools without a box
async function refresh() {
  toast(t('Downloading details…'));
  try { const r = await api<{ message: string }>('POST', `/library/metadata/${it.value.id}/refresh`); toast(t(r.message), 4000); emit('changed'); } catch (e) { fail(e); }
}
async function analyze(deep: boolean) {
  try { await api('POST', `/library/metadata/${it.value.id}/${deep ? 'deepanalyze' : 'analyze'}`); toast(t('Analyzing — progress is shown at the top')); poke(); } catch (e) { fail(e); }
}
async function unmatch() {
  box.value = '';
  try { const r = await api<{ message: string }>('DELETE', `/library/metadata/${it.value.id}/match`); toast(t(r.message), 4000); emit('changed'); } catch (e) { fail(e); }
}
async function cutTheme() {
  toast(t('Cutting the theme music…'));
  try {
    const r = await api<{ folder_theme?: string }>('POST', `/library/metadata/${it.value.id}/theme`);
    toast(r.folder_theme ? t('Theme music cut. The theme file in the show’s folder is played instead: {path}', { path: r.folder_theme }) : t('Theme music cut'), r.folder_theme ? 8000 : 3000);
  } catch (e) { toast(t((e as Error).message), 6000); }
}
async function split() {
  try { const r = await api<{ split: number }>('POST', `/library/metadata/${it.value.id}/split`); toast(t('{n} versions are separate titles again', { n: r.split })); emit('changed'); } catch (e) { fail(e); }
}

// ---- share: a link to this page, for someone with an account here
const link = computed(() => `${location.origin}${location.pathname}#/item/${it.value.id}`);
async function share() {
  if (navigator.share && matchMedia('(pointer: coarse)').matches) {
    try { await navigator.share({ title: name.value, url: link.value }); return; } catch (e) { if ((e as Error).name === 'AbortError') return; }
  }
  pick('share');
}
const linkInput = ref<HTMLInputElement | null>(null);
async function copyLink() {
  const inp = linkInput.value; if (!inp) return;
  inp.select();
  let done = false;
  try { if (navigator.clipboard && isSecureContext) { await navigator.clipboard.writeText(inp.value); done = true; } } catch { /* not allowed */ }
  if (!done) { try { done = document.execCommand('copy'); } catch { /* old browser */ } }
  toast(done ? t('Link copied') : t('Press Ctrl+C to copy the link'));
}

// ---- match: the right online entry when the automatic one was wrong or missing
interface Match { source: string, id: string, title: string, year?: number, thumb?: string, summary?: string, score: number }
const mTitle = ref(''), mYear = ref(''), matches = ref<Match[] | null>(null), mError = ref('');
async function search() {
  if (!mTitle.value && !matches.value) { mTitle.value = x.value.file_title || it.value.title; mYear.value = it.value.year ? String(it.value.year) : ''; }
  matches.value = null; mError.value = '';
  try { matches.value = (await get<{ matches: Match[] }>(`/library/metadata/${it.value.id}/matches?title=${encodeURIComponent(mTitle.value)}&year=${encodeURIComponent(mYear.value)}`)).matches; }
  catch (e) { mError.value = (e as Error).message; }
}
async function choose(c: Match) {
  // The box closes at once; the details come in the background and the page is drawn again when they are there.
  box.value = '';
  toast(t('Downloading details for {title}…', { title: c.title }), 60000);
  try { const a = await api<{ message: string }>('PUT', `/library/metadata/${it.value.id}/match`, { source: c.source, id: c.id }); toast(t(a.message), 4000); emit('changed'); } catch (e) { fail(e); }
}

// ---- optimize: a copy every browser plays as it is
const KINDS: [string, string][] = [['tv', 'Optimized for TV (1080p, 8 Mbps)'], ['mobile', 'Optimized for mobile (720p, 2 Mbps)'], ['original', 'Original quality (same size, up to 20 Mbps)']];
const quality = ref('tv');
const optimized = computed<Any | null>(() => x.value.optimized || null);
async function optimize() {
  try { const r = await api<{ videos: number }>('POST', `/library/metadata/${it.value.id}/optimize`, { quality: quality.value }); toast(t('Optimizing {n} video(s) — progress is shown at the top', { n: r.videos }), 5000); box.value = ''; poke(); } catch (e) { fail(e); }
}
const sureOptDelete = ref(false);
async function deleteOptimized() {
  try { const r = await api<{ removed: number }>('DELETE', `/library/metadata/${it.value.id}/optimize`); toast(t('{n} optimized version(s) deleted', { n: r.removed })); box.value = ''; sureOptDelete.value = false; emit('changed'); } catch (e) { fail(e); }
}

// ---- play history
interface Seen { at: number, user?: string, manual?: boolean, item: Item }
const history = ref<Seen[] | null>(null);
async function loadHistory() {
  history.value = null;
  try { history.value = (await get<{ history: Seen[] }>(`/library/metadata/${it.value.id}/history`)).history; } catch (e) { fail(e); box.value = ''; }
}
const epName = (e: Item) => `${t('S{n}', { n: e.parentIndex || 0 })}${t('E{n}', { n: e.index || 0 })} · ${e.title}`;

// ---- media info: every stream of the file
const media = computed<Any | null>(() => ((it.value.Media || [])[0] as Any) || null);
const STREAM: Record<string, string> = { video: 'Video', audio: 'Audio', subtitle: 'Subtitles' };
const hdr = (s: Any) => (s.dovi ? 'Dolby Vision' : s.hdr10plus ? 'HDR10+' : s.color_transfer === 'smpte2084' ? 'HDR10' : s.color_transfer === 'arib-std-b67' ? 'HLG' : '');
const shown = (rows: (null | [string, unknown])[]) => rows.filter((r): r is [string, unknown] => !!r && r[1] !== undefined && r[1] !== null && r[1] !== '' && r[1] !== 0);
// What deep analysis found: from the slowest connection level to the fastest.
function neededSpeed(m: Any) {
  const k = String(m.requiredBandwidths || '').split(',').map(Number).filter(n => n > 0 && n < 2147483647);
  if (!k.length) return t('Not measured yet — use Deep analysis');
  const mbit = (n: number) => fmtNum(n / 1000, 1);
  const a = Math.max(...k), b = Math.min(...k);
  return a === b ? mbit(a) + ' Mbit/s' : `${mbit(b)}–${mbit(a)} Mbit/s`;
}
const fileRows = computed(() => {
  const m = media.value; if (!m) return [];
  const opt = optimized.value;
  return shown([[t('File'), m.file], [t('File type'), String(m.container || '').toUpperCase()], [t('Size'), m.size ? fmtSize(m.size) : ''],
    [t('Length'), fmtDur(it.value.duration)], [t('Bitrate'), m.bitrate ? fmtNum(m.bitrate / 1000, 1) + ' Mbit/s' : ''],
    [t('Connection speed needed'), neededSpeed(m)],
    (m.Part || []).length > 1 ? [t('Parts'), t('{n} files, played one after the other', { n: m.Part.length })] : null,
    opt ? [t('Optimized version'), `${opt.quality}, ${fmtSize(opt.size || 0)}`] : null,
    [t('Subtitle files'), (m.subtitles || []).filter((s: Any) => !s.embedded).map((s: Any) => s.file).join(', ')]]);
});
const streamRows = (s: Any) => shown([[t('Codec'), (s.codec || '') + (s.profile ? ` (${s.profile})` : '')], [t('Language'), s.language], [t('Title'), s.title],
  s.width ? [t('Resolution'), `${s.width}×${s.height}`] : null, [t('Bit depth'), s.bit_depth ? t('{n}-bit', { n: s.bit_depth }) : ''],
  [t('HDR'), hdr(s)], [t('Channels'), s.channels ? `${s.channels}${s.channel_layout ? ' (' + s.channel_layout + ')' : ''}` : '']]);

// ---- delete the file(s) from disk, confirmed by typing the name
const typed = ref('');
const word = computed(() => (it.value.type === 'episode' ? it.value.title : name.value));
const deleteText = computed(() => (isPlayable(it.value)
  ? t('Delete “{title}” from disk? The file is deleted for good; this cannot be undone.', { title: name.value })
  : t('Delete all {n} episodes of “{title}” from disk? The files are deleted for good; this cannot be undone.', { n: it.value.leafCount || 0, title: name.value })));
async function remove() {
  if (typed.value.trim() !== word.value) return;
  try {
    const r = await api<{ deleted: number }>('DELETE', '/library/metadata/' + it.value.id);
    toast(t('{n} file(s) deleted', { n: r.deleted }), 4000);
    box.value = '';
    // Its page no longer exists: back to its show or season if that is still there, else to the library.
    const up = it.value.parentRatingKey || it.value.grandparentRatingKey;
    const still = up ? list<Item>(await get('/library/metadata/' + up).catch(() => null))[0] : null;
    router.replace(still ? '/item/' + still.id : '/library/' + it.value.librarySectionID);
  } catch (e) { fail(e); }
}
</script>

<template>
  <span class="menu-wrap" :class="{ 'card-menu': card }">
    <button v-if="card" ref="btn" type="button" class="more" :class="{ open }" :title="t('More')" :aria-label="t('More')" aria-haspopup="menu" :aria-expanded="open" @click.prevent.stop="toggle">⋯</button>
    <button v-else ref="btn" class="iconbtn round" :title="t('More')" :aria-label="t('More')" aria-haspopup="menu" :aria-expanded="open" @click="toggle"><Icon name="more" /></button>
    <Teleport to="body" :disabled="!card">
    <div v-if="open" class="menu item-menu" :class="{ 'side-menu': card }" role="menu" :style="card ? { left: at.left + 'px', top: at.top + 'px' } : undefined">
      <template v-if="card">
        <button v-if="selectable" class="menu-row" role="menuitem" @click="pick(() => emit('select'))">{{ t('Select') }}</button>
        <button v-if="cont" class="menu-row" role="menuitem" @click="pick(uncontinue)">{{ t('Remove from Continue Watching') }}</button>
        <button v-if="it.type !== 'clip'" class="menu-row" role="menuitem" @click="pick(watchlist)">{{ it.watchlist ? t('Remove from watchlist') : t('Add to watchlist') }}</button>
        <button class="menu-row" role="menuitem" @click="pick('addto')">{{ t('Add to …') }}</button>
        <button class="menu-row" role="menuitem" @click="pick(() => markWatched(!isWatched(it)))">{{ isWatched(it) ? t('Mark as unwatched') : t('Mark as watched') }}</button>
        <button v-if="started" class="menu-row" role="menuitem" @click="pick(() => markWatched(false))">{{ t('Mark as unwatched') }}</button>
        <hr>
      </template>
      <button class="menu-row" role="menuitem" @click="share">{{ t('Share') }}</button>
      <button v-if="it.type !== 'clip' && !it.missing" class="menu-row" role="menuitem" @click="pick(() => addToQueue(it))">{{ t('Add to queue') }}</button>
      <template v-if="admin">
        <hr>
        <button class="menu-row" role="menuitem" @click="pick(refresh)">{{ t('Refresh metadata') }}</button>
        <button class="menu-row" role="menuitem" @click="pick(() => analyze(false))">{{ t('Analyze') }}</button>
        <button class="menu-row" role="menuitem" @click="pick(() => analyze(true))">{{ t('Deep analysis') }}</button>
        <button v-if="matchable" class="menu-row" role="menuitem" @click="pick('match')">{{ x.metadata_source ? t('Fix match…') : t('Match…') }}</button>
        <button v-if="matchable && x.metadata_source" class="menu-row" role="menuitem" @click="pick('unmatch')">{{ t('Unmatch') }}</button>
        <button class="menu-row" role="menuitem" @click="pick('optimize')">{{ t('Optimize…') }}</button>
        <button v-if="it.type === 'show'" class="menu-row" role="menuitem" @click="pick(cutTheme)">{{ t('Cut theme music') }}</button>
        <button v-if="(it.Media || []).length > 1" class="menu-row" role="menuitem" @click="pick(split)">{{ t('Split the merged versions') }}</button>
        <button v-if="it.librarySectionID" class="menu-row" role="menuitem" @click="pick('access')">{{ t('Grant access…') }}</button>
      </template>
      <hr>
      <button class="menu-row" role="menuitem" @click="pick('history')">{{ t('View play history') }}</button>
      <button v-if="isPlayable(it)" class="menu-row" role="menuitem" @click="pick('info')">{{ t('Media info') }}</button>
      <template v-if="deletable">
        <hr>
        <button class="menu-row danger-text" role="menuitem" @click="typed = ''; pick('delete')">{{ t('Delete…') }}</button>
      </template>
    </div>
    </Teleport>
    <AddToDialog v-if="box === 'addto'" :item="it" @close="box = ''" @changed="emit('changed')" />

    <Modal v-if="box === 'share'" :title="t('Share “{title}”', { title: name })" @close="box = ''">
      <p class="dim">{{ t('Anyone with an account on this server who may see this library can open it.') }}</p>
      <input ref="linkInput" :value="link" class="full" readonly @focus="($event.target as HTMLInputElement).select()">
      <template #actions>
        <button class="primary" @click="copyLink">{{ t('Copy link') }}</button>
        <button @click="box = ''">{{ t('Close') }}</button>
      </template>
    </Modal>

    <Modal v-if="box === 'unmatch'" :title="t('Unmatch')" @close="box = ''">
      <p>{{ t('Remove the online details of "{title}"? It goes back to the file name and is not matched automatically again until you pick a match.', { title: it.title }) }}</p>
      <template #actions>
        <button class="danger" @click="unmatch">{{ t('Unmatch') }}</button>
        <button @click="box = ''">{{ t('Cancel') }}</button>
      </template>
    </Modal>

    <Modal v-if="box === 'match'" :title="x.metadata_source ? t('Fix match') : t('Match')" wide @close="box = ''">
      <form class="form" @submit.prevent="search">
        <label>{{ t('Title') }}</label><div class="val"><input v-model="mTitle" class="full"></div>
        <label>{{ t('Year') }}</label><div class="val"><input v-model="mYear" class="short"></div>
        <span></span><div class="val"><button class="primary" type="submit">{{ t('Search') }}</button></div>
      </form>
      <p v-if="mError" class="err">{{ mError }}</p>
      <p v-else-if="matches === null" class="dim">{{ t('Searching…') }}</p>
      <p v-else-if="!matches.length" class="dim">{{ t('Nothing found. Try another spelling or leave the year empty.') }}</p>
      <div v-else class="pick-list">
        <button v-for="c in matches" :key="c.source + c.id" class="pick match-pick" @click="choose(c)">
          <img v-if="c.thumb" :src="c.thumb" alt="" referrerpolicy="no-referrer"><span v-else class="initials">{{ c.title[0] || '?' }}</span>
          <span class="grow">
            <b>{{ c.title }}</b><template v-if="c.year"> ({{ c.year }})</template>
            <span class="dim block">{{ c.source }} · {{ t('fit {n}%', { n: Math.round(Math.max(0, Math.min(1, c.score)) * 100) }) }}</span>
            <span v-if="c.summary" class="ep-plot">{{ c.summary }}</span>
          </span>
        </button>
      </div>
      <template #actions><button @click="box = ''">{{ t('Close') }}</button></template>
    </Modal>

    <Modal v-if="box === 'optimize'" :title="t('Optimize “{title}”', { title: it.title })" @close="box = ''; sureOptDelete = false">
      <p class="dim">{{ t('Saves a copy as MP4 (H.264 video, stereo AAC sound) that browsers, phones and TVs play directly. It is played instead of the original whenever the original would have to be converted, which saves the server work while watching. Copies are kept in the server’s data folder (optimized).') }}</p>
      <p v-if="!isPlayable(it)" class="dim">{{ t('Every episode is optimized, one after the other.') }}</p>
      <p v-if="optimized">{{ t('Already optimized: {kind}, {size}.', { kind: t((KINDS.find(k => k[0] === optimized!.quality) || ['', optimized.quality])[1]), size: fmtSize(optimized.size || 0) }) }}</p>
      <div class="form">
        <label>{{ t('Quality') }}</label>
        <div class="val"><select v-model="quality"><option v-for="[v, l] in KINDS" :key="v" :value="v">{{ t(l) }}</option></select></div>
      </div>
      <p v-if="sureOptDelete" class="err">{{ t('Delete the optimized versions of this video?') }}</p>
      <template #actions>
        <template v-if="sureOptDelete">
          <button class="danger" @click="deleteOptimized">{{ t('Delete') }}</button>
          <button @click="sureOptDelete = false">{{ t('Cancel') }}</button>
        </template>
        <template v-else>
          <button class="primary" @click="optimize">{{ t('Optimize') }}</button>
          <button v-if="optimized || !isPlayable(it)" class="danger" @click="sureOptDelete = true">{{ t('Delete optimized versions') }}</button>
          <button @click="box = ''">{{ t('Cancel') }}</button>
        </template>
      </template>
    </Modal>

    <AccessDialog v-if="box === 'access' && it.librarySectionID" :section="Number(it.librarySectionID)" for-item @close="box = ''" />

    <Modal v-if="box === 'history'" :title="t('Play history: {title}', { title: it.title })" @close="box = ''">
      <div v-if="history === null" class="skel line"></div>
      <div v-else-if="history.length" class="table-wrap">
        <table class="table">
          <tr><th>{{ t('When') }}</th><th v-if="admin">{{ t('Who') }}</th><th v-if="!isPlayable(it)">{{ t('Episode') }}</th><th></th></tr>
          <tr v-for="(e, i) in history" :key="i">
            <td class="nowrap">{{ fmtDateTime(e.at) }}</td>
            <td v-if="admin">{{ e.user || '–' }}</td>
            <td v-if="!isPlayable(it)"><RouterLink :to="'/item/' + e.item.id" @click="box = ''">{{ epName(e.item) }}</RouterLink></td>
            <td class="dim">{{ e.manual ? t('marked as watched') : t('watched') }}</td>
          </tr>
        </table>
      </div>
      <p v-else class="dim">{{ t('Not watched yet.') }}</p>
      <template #actions><button @click="box = ''">{{ t('Close') }}</button></template>
    </Modal>

    <Modal v-if="box === 'info'" :title="t('Media info')" wide @close="box = ''">
      <p v-if="!media" class="dim">{{ t('This entry has no video file') }}</p>
      <template v-else>
        <div class="kv"><template v-for="[k, v] in fileRows" :key="k"><div class="dim">{{ k }}</div><div class="break">{{ v }}</div></template></div>
        <p v-if="media.probed === false" class="dim">{{ t('The file has not been read with ffprobe yet — use Analyze.') }}</p>
        <template v-for="s in (media.streams || [])" :key="s.kind + s.index">
          <h3 class="gap-top">{{ t(STREAM[s.kind] || s.kind) }} #{{ s.index }}<template v-if="s.default"> · {{ t('default') }}</template></h3>
          <div class="kv"><template v-for="[k, v] in streamRows(s)" :key="k"><div class="dim">{{ k }}</div><div class="break">{{ v }}</div></template></div>
        </template>
      </template>
      <template #actions><button @click="box = ''">{{ t('Close') }}</button></template>
    </Modal>

    <Modal v-if="box === 'delete'" :title="deleteText" @close="box = ''">
      <p class="dim">{{ t('Type “{name}” to confirm.', { name: word }) }}</p>
      <form @submit.prevent="remove"><input v-model="typed" class="full" :placeholder="word" autocomplete="off"></form>
      <template #actions>
        <button class="danger" :disabled="typed.trim() !== word" @click="remove">{{ t('Delete') }}</button>
        <button @click="box = ''">{{ t('Cancel') }}</button>
      </template>
    </Modal>
  </span>
</template>
