<script setup lang="ts">
// One plugin's own panel (POST /api/plugins/{id}/call/panel): a title, text, a search box, a file upload,
// a list of entries with their buttons, and buttons. A button calls the plugin's action; its answer can
// bring a new panel, a message, or a code to type in somewhere else, which is then asked about again
// until it is done (a device login).
import { onBeforeUnmount, ref } from 'vue';
import { api, picture } from '../api';
import { lang, t } from '../i18n';
import { toast } from '../toast';
import Modal from './Modal.vue';

interface Button { label: string, action: string, args?: unknown, primary?: boolean, danger?: boolean, confirm?: string, working?: string }
interface Entry { title?: string, subtitle?: string, href?: string, image?: string, status?: string, status_kind?: string, buttons?: Button[] }
interface Panel {
  title?: string, text?: string, link?: string, status?: string, buttons?: Button[], items?: Entry[],
  input?: { action: string, placeholder?: string, value?: string, label?: string, args?: unknown },
  upload?: { action: string, accept?: string, label?: string },
}
interface Answer {
  panel?: Panel, message?: string, error?: string, poll?: { action: string, seconds?: number }, code?: string, link?: string, wait?: string,
  marked?: number, matched?: number, rated?: number, watchlisted?: number, playlist_items?: number,
}
const props = defineProps<{ id: string, name: string }>();
const p = ref<Panel | null>(null);
const busy = ref(''); // the label of the button that runs
const query = ref('');

const call = <T,>(action: string, extra?: Record<string, unknown>) => api<T>('POST', `/api/plugins/${props.id}/call/${action}`, { lang: lang.value, ...(extra || {}) });
async function load() {
  try { p.value = await call<Panel>('panel'); } catch (e) { p.value = { title: props.name, text: (e as Error).message }; }
}
load();

function after(r: Answer, label: string) {
  if (r.poll) { startPoll(label, r); return; }
  const counts = (r.marked !== undefined ? ' ' + t('{found} found in your libraries, {marked} newly marked as watched.', { found: r.matched ?? 0, marked: r.marked }) : '')
    + (r.rated !== undefined ? ' ' + t('{n} ratings set.', { n: r.rated }) : '')
    + (r.watchlisted !== undefined ? ' ' + t('{n} put on your watchlist.', { n: r.watchlisted }) : '')
    + (r.playlist_items !== undefined ? ' ' + t('Playlist with {n} titles saved.', { n: r.playlist_items }) : '');
  if (r.error) toast(r.error, 6000); else if (r.message || counts) toast((r.message || '') + counts, 8000);
  if (r.panel) p.value = r.panel; else load();
}
const asking = ref<Button | null>(null);
async function run(b: Button, sure = false) {
  if (b.confirm && !sure) { asking.value = b; return; }
  asking.value = null;
  busy.value = b.label;
  try { after(await call<Answer>(b.action, b.args ? { args: b.args } : undefined), b.label); } catch (e) { toast((e as Error).message, 6000); } finally { busy.value = ''; }
}
async function search() {
  const i = p.value && p.value.input; if (!i || !query.value.trim()) return;
  busy.value = '\u0000search';
  try { after(await call<Answer>(i.action, { query: query.value.trim(), ...(i.args ? { args: i.args } : {}) }), i.label || ''); } catch (e) { toast((e as Error).message, 6000); } finally { busy.value = ''; }
}
// A file (an export from elsewhere) goes to the plugin as base64.
const file = ref<HTMLInputElement | null>(null);
async function upload() {
  const u = p.value && p.value.upload, f = file.value?.files?.[0];
  if (!u || !f) return;
  if (f.size > 2.5 * 1024 * 1024) { toast(t('The file is too big (at most 2.5 MB).'), 6000); return; }
  busy.value = '\u0000upload';
  try {
    const data = await new Promise<string>((ok, no) => { const rd = new FileReader(); rd.onload = () => ok(String(rd.result).split(',')[1] || ''); rd.onerror = no; rd.readAsDataURL(f); });
    after(await call<Answer>(u.action, { file: data, name: f.name }), u.label || '');
  } catch (e) { toast((e as Error).message, 6000); } finally { busy.value = ''; }
}
// Pictures only from the web (https) or the server itself (with the ticket, or the server turns them away).
const pic = (src?: string) => (typeof src === 'string' && /^(https:\/\/|\/(?!\/))/.test(src) ? (src[0] === '/' ? picture(src, 120) : src) : '');
const hasPics = () => (p.value?.items || []).some(it => 'image' in it);
// A title may link to a page of the app (the classic app's address), or to a page on the web.
const inApp = (href?: string) => typeof href === 'string' && /^#\/[\w/-]+$/.test(href);
const onWeb = (href?: string) => typeof href === 'string' && /^https:\/\/[^\s"<>]+$/.test(href);
const appLink = (href: string) => href.replace(/^#\/section\//, '/library/').replace(/^#/, '');

// ---- a code to type in somewhere else: asked about again until it is done
const poll = ref<{ title: string, message: string, code: string, link: string, wait: string } | null>(null);
let pollTimer = 0;
function startPoll(title: string, first: Answer) {
  poll.value = { title, message: '', code: '', link: '', wait: '' };
  next(first);
}
function next(r: Answer) {
  const box = poll.value; if (!box) return;
  if (r.message) box.message = r.message;
  if (r.code) box.code = r.code;
  if (r.link) box.link = r.link;
  if (!r.poll) { stopPoll(); if (r.error) toast(r.error, 6000); else if (r.message) toast(r.message, 5000); return; }
  box.wait = r.wait || t('Waiting until the code is entered…');
  const action = r.poll.action;
  pollTimer = window.setTimeout(async () => {
    try { next(await call<Answer>(action)); } catch (e) { stopPoll(); toast((e as Error).message, 6000); }
  }, Math.max(2, r.poll.seconds || 5) * 1000);
}
function stopPoll() { clearTimeout(pollTimer); if (poll.value) { poll.value = null; load(); } }
onBeforeUnmount(() => clearTimeout(pollTimer));
</script>

<template>
  <section class="panel">
    <p v-if="!p" class="dim">{{ t('Loading…') }}</p>
    <template v-else>
      <h2>{{ p.title || name }}<span v-if="p.status === 'on'" class="ok small-tag">{{ t('connected') }}</span></h2>
      <p v-if="p.text" class="pre">{{ p.text }}</p>
      <p v-if="p.link"><a :href="p.link" target="_blank" rel="noopener">{{ p.link }}</a></p>
      <form v-if="p.input && p.input.action" class="pp-search" @submit.prevent="search">
        <input v-model="query" type="search" :placeholder="p.input.placeholder || ''">
        <button class="primary" type="submit" :disabled="!!busy">{{ p.input.label || t('Search') }}</button>
      </form>
      <form v-if="p.upload && p.upload.action" class="pp-search" @submit.prevent="upload">
        <input ref="file" type="file" :accept="p.upload.accept || ''">
        <button class="primary" type="submit" :disabled="!!busy">{{ p.upload.label || t('Upload') }}</button>
      </form>
      <div v-if="p.items && p.items.length" class="pp-items">
        <div v-for="(it, i) in p.items" :key="i" class="pp-item">
          <template v-if="hasPics()"><img v-if="pic(it.image)" :src="pic(it.image)" alt="" loading="lazy" referrerpolicy="no-referrer"><div v-else class="pp-noimg"></div></template>
          <div class="grow">
            <RouterLink v-if="inApp(it.href)" :to="appLink(it.href!)"><b>{{ it.title }}</b></RouterLink>
            <a v-else-if="onWeb(it.href)" :href="it.href" target="_blank" rel="noopener noreferrer"><b>{{ it.title }}</b></a>
            <b v-else>{{ it.title }}</b>
            <span v-if="it.status" class="tag" :class="it.status_kind">{{ it.status }}</span>
            <div v-if="it.subtitle" class="dim">{{ it.subtitle }}</div>
          </div>
          <div v-if="it.buttons && it.buttons.length" class="actions">
            <button v-for="b in it.buttons" :key="b.label" class="small" :class="{ primary: b.primary }" :disabled="!!busy" @click="run(b)">{{ busy === b.label ? b.working || t('Working…') : b.label }}</button>
          </div>
        </div>
      </div>
      <div v-if="p.buttons && p.buttons.length" class="actions">
        <button v-for="b in p.buttons" :key="b.label" :class="{ primary: b.primary }" :disabled="!!busy" @click="run(b)">{{ busy === b.label ? b.working || t('Working…') : b.label }}</button>
      </div>
    </template>
    <Modal v-if="asking" :title="asking.confirm || ''" @close="asking = null">
      <template #actions>
        <button :class="asking.action === 'disconnect' || asking.danger ? 'danger' : 'primary'" @click="run(asking, true)">{{ t('OK') }}</button>
        <button @click="asking = null">{{ t('Cancel') }}</button>
      </template>
    </Modal>
    <Modal v-if="poll" :title="poll.title" @close="stopPoll">
      <p>{{ poll.message }}</p>
      <div class="big-code select">{{ poll.code }}</div>
      <p v-if="poll.link"><a :href="poll.link" target="_blank" rel="noopener">{{ poll.link }}</a></p>
      <p class="dim">{{ poll.wait }}</p>
      <template #actions><button @click="stopPoll">{{ t('Cancel') }}</button></template>
    </Modal>
  </section>
</template>
