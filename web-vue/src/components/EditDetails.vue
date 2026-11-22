<script setup lang="ts">
// "Edit details" (owner): the same tabs as in the classic app. General, Tags and Labels (lists typed comma
// separated), Markers (intro and credits, h:mm:ss), the pictures, Advanced and Info. Saved with
// PUT /library/metadata/{id}; markers only when they were changed (PUT …/markers).
import { computed, reactive, ref } from 'vue';
import { api, get, list, picture, type Item } from '../api';
import { fmtTime } from '../format';
import { lang, t } from '../i18n';
import Modal from './Modal.vue';

const props = defineProps<{ id: number }>();
const emit = defineEmits<{ close: [], saved: [] }>();

type Full = Omit<Item, 'Media' | 'extra'> & { titleSort?: string, addedAt?: number, Media?: { file?: string }[],
  extra?: { directors?: string[], writers?: string[], collections?: string[], original_title?: string, edition?: string, originally_available?: string, countries?: string[], producers?: string[],
    labels?: string[], language_override?: string, credits_detection?: string, use_original_title?: string } };
interface Markers { intro?: number[], credits?: number[], introSource?: string, source?: string, errors?: number, watchedAt: number, duration: number }

const it = ref<Full | null>(null);
const mk = ref<Markers | null>(null);
const langs = ref<{ code: string, name: string }[]>([]);
const tab = ref('general');
const error = ref('');
const busy = ref(false);
const f = reactive<Record<string, string>>({});
const m = reactive({ is: '', ie: '', cs: '' });
let mInit = { is: '', ie: '', cs: '' };
const pics = reactive<Record<string, number>>({}); // a picture that changed: shown again (cache-busting)

const joined = (v?: string[]) => (v || []).join(', ');
const clock = (ms?: number) => (ms || ms === 0 ? fmtTime(ms / 1000) : '');
function unclock(v: string): number | null {
  const p = v.trim().split(':').map(Number);
  if (!v.trim() || p.some(n => isNaN(n) || n < 0)) return null;
  return p.reduce((a, b) => a * 60 + b, 0) * 1000;
}

const movie = computed(() => !!it.value && (it.value.type === 'movie' || it.value.type === 'episode'));
(async () => {
  try {
    const i = list<Full>(await get('/library/metadata/' + props.id))[0];
    if (!i) { error.value = t('Not found'); return; }
    const x = i.extra || {};
    Object.assign(f, {
      title: i.title || '', sort_title: i.titleSort || '', original_title: x.original_title || '', edition: x.edition || '',
      originally_available: (x.originally_available || '').slice(0, 10), year: i.year ? String(i.year) : '', content_rating: i.contentRating || '',
      rating: i.rating ? String(i.rating) : '', studio: i.studio || '', tagline: i.tagline || '', summary: i.summary || '',
      directors: joined(x.directors), countries: joined(x.countries), genres: joined(i.genres), writers: joined(x.writers),
      producers: joined(x.producers), collections: joined(x.collections), labels: joined(x.labels),
      language_override: x.language_override || '', credits_detection: x.credits_detection || '', use_original_title: x.use_original_title || '',
    });
    it.value = i;
    const [s, k] = await Promise.all([
      get<{ languages?: { code: string, name: string }[] }>('/api/settings').catch(() => null),
      movie.value ? get<Markers>(`/library/metadata/${i.id}/markers`).catch(() => null) : Promise.resolve(null),
    ]);
    langs.value = (s && s.languages) || [];
    if (k) {
      mInit = { cs: clock((k.credits || [])[0]), is: clock((k.intro || [])[0]), ie: clock((k.intro || [])[1]) };
      Object.assign(m, mInit);
      mk.value = k;
    }
  } catch (e) { error.value = (e as Error).message; }
})();

const tabs = computed(() => [['general', t('General')], ['tags', t('Tags')], ['labels', t('Labels')], ...(mk.value ? [['markers', t('Markers')]] : []),
  ['pictures', t('Pictures')], ['advanced', t('Advanced')], ['info', t('Info')]]);
const GENERAL: [string, string, string?][] = [['title', 'Title'], ['sort_title', 'Sort title'], ['original_title', 'Original title'], ['edition', 'Edition (e.g. Director’s Cut)'],
  ['originally_available', 'Originally available', 'date'], ['year', 'Year', 'number'], ['content_rating', 'Age rating'], ['rating', 'Rating (0-10)'], ['studio', 'Studio'], ['tagline', 'Tagline']];
const TAGS: [string, string][] = [['directors', 'Directors'], ['countries', 'Countries'], ['genres', 'Genres'], ['writers', 'Writers'], ['producers', 'Producers'], ['collections', 'Collections']];
const srcText = (src?: string, errors?: number) => (({ manual: t('set by hand'), chapters: t('from a chapter named Credits in the file'), picture: t('found in the picture (dark screen with text near the end)'),
  sound: t('found by comparing the sound of the episodes'), none: t('none found'), pending: t('not looked for yet (detection runs in the background)') } as Record<string, string>)[src || ''] || src || '')
  + (errors ? t('; detection failed {n} time(s)', { n: errors }) : '');
const fmtAt = (ts?: number) => (ts ? new Date(ts * (ts < 1e12 ? 1000 : 1)).toLocaleString(lang.value) : '');

// The pictures: poster, background, logo and square poster.
const slots = computed(() => {
  const i = it.value; if (!i) return [];
  return [['poster', t('Poster'), `/library/metadata/${i.id}/thumb`], ['art', t('Background'), `/library/metadata/${i.id}/art`],
    ['logo', t('Logo'), `/library/metadata/${i.id}/logo`], ['square', t('Square poster'), `/library/metadata/${i.id}/square`]];
});
const missing = reactive<Record<string, boolean>>({});
const addr = reactive<Record<string, string>>({});
const note = ref('');
async function sendPic(slot: string, body: unknown) {
  try { await api('PUT', `/library/metadata/${props.id}/image/${slot}`, body); pics[slot] = Date.now(); missing[slot] = false; note.value = t('Picture saved'); }
  catch (e) { note.value = (e as Error).message; }
}
function upload(slot: string, e: Event) {
  const file = (e.target as HTMLInputElement).files?.[0]; if (!file) return;
  if (file.size > 3 * 1024 * 1024) { note.value = t('The picture is too big (3 MB at most)'); return; }
  const r = new FileReader(); r.onload = () => sendPic(slot, { data: r.result }); r.readAsDataURL(file);
}
async function removePic(slot: string) {
  try { await api('DELETE', `/library/metadata/${props.id}/image/${slot}`); missing[slot] = true; note.value = t('Picture removed'); }
  catch (e) { note.value = (e as Error).message; }
}
async function detectCredits() {
  try { await api('POST', `/library/metadata/${props.id}/credits`); note.value = t('Detecting credits…'); } catch (e) { note.value = (e as Error).message; }
}

async function save() {
  error.value = '';
  const body: Record<string, unknown> = { ...f };
  body.year = f.year ? Number(f.year) : null;
  body.rating = f.rating ? Number(String(f.rating).replace(',', '.')) : null;
  if (!f.sort_title.trim()) delete body.sort_title;
  if (!movie.value) delete body.edition;
  // Only markers that were changed are sent; emptied ones go back to what was found (only a hand-set one can be taken back).
  const mb: Record<string, unknown> = {}, k = mk.value;
  if (k) {
    const cs = m.cs.trim(), is = m.is.trim(), ie = m.ie.trim();
    if (cs !== mInit.cs) {
      if (cs) { const ms = unclock(cs); if (ms === null) { error.value = t('Enter a time like 1:52:30'); tab.value = 'markers'; return; } mb.credits = [ms]; }
      else if (k.source === 'manual') mb.credits = null;
    }
    if (is !== mInit.is || ie !== mInit.ie) {
      if (!is && !ie) { if (k.introSource === 'manual') mb.intro = null; }
      else {
        const a = unclock(is), b = unclock(ie);
        if (a === null || b === null || b <= a) { error.value = t('Enter the intro start and end, the end after the start'); tab.value = 'markers'; return; }
        mb.intro = [a, b];
      }
    }
  }
  busy.value = true;
  try {
    await api('PUT', '/library/metadata/' + props.id, body);
    if (Object.keys(mb).length) await api('PUT', `/library/metadata/${props.id}/markers`, mb);
    emit('saved'); emit('close');
  } catch (e) { error.value = (e as Error).message; busy.value = false; }
}
</script>

<template>
  <Modal :title="t('Edit details')" wide @close="emit('close')">
    <p v-if="!it && !error" class="dim">{{ t('Loading…') }}</p>
    <template v-if="it">
      <div class="tabs flat">
        <button v-for="[k, name] in tabs" :key="k" :class="{ on: tab === k }" @click="tab = k">{{ name }}</button>
      </div>
      <div v-if="tab === 'general'" class="form">
        <template v-for="[k, label, type] in GENERAL" :key="k">
          <template v-if="k !== 'edition' || movie">
            <label :for="'ed-' + k">{{ t(label) }}</label>
            <div><input :id="'ed-' + k" v-model="f[k]" :type="type || 'text'"><div v-if="k === 'sort_title'" class="dim small">{{ t('Leave empty to derive it from the title.') }}</div></div>
          </template>
        </template>
        <label for="ed-summary">{{ t('Summary') }}</label>
        <textarea id="ed-summary" v-model="f.summary" rows="6"></textarea>
      </div>
      <div v-else-if="tab === 'tags'" class="form">
        <template v-for="[k, label] in TAGS" :key="k"><label :for="'ed-' + k">{{ t(label) }}</label><input :id="'ed-' + k" v-model="f[k]"></template>
      </div>
      <div v-else-if="tab === 'labels'" class="form">
        <label for="ed-labels">{{ t('Labels (comma separated)') }}</label><input id="ed-labels" v-model="f.labels">
      </div>
      <div v-else-if="tab === 'markers' && mk">
        <p class="dim">{{ t('Intro: {from}.', { from: srcText(mk.introSource) }) }}<br>
          {{ t('Credits: {from}. This video counts as watched at {at} of {length}.', { from: srcText(mk.source, mk.errors), at: fmtTime(mk.watchedAt / 1000), length: fmtTime(mk.duration / 1000) }) }}</p>
        <div class="form">
          <label for="ed-is">{{ t('Intro starts at (hours:minutes:seconds)') }}</label><input id="ed-is" v-model="m.is" :placeholder="t('e.g. 0:00:45')">
          <label for="ed-ie">{{ t('Intro ends at') }}</label><div><input id="ed-ie" v-model="m.ie" :placeholder="t('e.g. 0:02:10')"><div class="dim small">{{ t('Empty both to go back to the found intro.') }}</div></div>
          <label for="ed-cs">{{ t('Credits start at') }}</label><div><input id="ed-cs" v-model="m.cs" :placeholder="t('e.g. 1:52:30')"><div class="dim small">{{ t('Empty to use the file’s chapters again.') }}</div></div>
        </div>
        <p><button :title="t('Read the chapters of this file again and look for the end credits')" @click="detectCredits">{{ t('Detect credits again') }}</button></p>
      </div>
      <div v-else-if="tab === 'pictures'" class="pics">
        <div v-for="[slot, label, url] in slots" :key="slot" class="pic">
          <b>{{ label }}</b>
          <img v-if="!missing[slot]" :src="picture(url + '?v=' + (pics[slot] || it.updatedAt || 0))" alt="" @error="missing[slot] = true">
          <div v-else class="dim">{{ t('No picture') }}</div>
          <div class="actions">
            <label class="btn">{{ t('Upload a picture') }}<input type="file" accept="image/jpeg,image/png,image/webp,image/gif" hidden @change="upload(slot, $event)"></label>
            <button v-if="!missing[slot]" class="danger" @click="removePic(slot)">{{ t('Remove') }}</button>
          </div>
          <div class="addr"><input v-model="addr[slot]" placeholder="https://…" :aria-label="t('From an address')">
            <button :disabled="!(addr[slot] || '').trim()" @click="sendPic(slot, { url: addr[slot].trim() })">{{ t('Use this address') }}</button></div>
        </div>
      </div>
      <div v-else-if="tab === 'advanced'" class="form">
        <label for="ed-lang">{{ t('Language for details') }}</label>
        <div><select id="ed-lang" v-model="f.language_override"><option value="">{{ t('Library default') }}</option><option v-for="l in langs" :key="l.code" :value="l.code">{{ l.name }}</option></select>
          <div class="dim small">{{ t('Applies the next time the details are fetched online (menu "⋯" → refresh).') }}</div></div>
        <label for="ed-cd">{{ t('Detect credits') }}</label>
        <div><select id="ed-cd" v-model="f.credits_detection"><option value="">{{ t('Library default') }}</option><option value="on">{{ t('Yes') }}</option><option value="off">{{ t('No') }}</option></select>
          <div class="dim small">{{ t('Whether the "Skip credits" mark is searched for in this title.') }}</div></div>
        <label for="ed-uo">{{ t('Use original title') }}</label>
        <div><select id="ed-uo" v-model="f.use_original_title"><option value="">{{ t('Library default') }}</option><option value="yes">{{ t('Yes') }}</option><option value="no">{{ t('No') }}</option></select>
          <div class="dim small">{{ t('Shows the original title instead of the translated one.') }}</div></div>
      </div>
      <div v-else class="kv">
        <div class="dim">ID</div><div>{{ it.id }}</div>
        <div class="dim">{{ t('Type') }}</div><div>{{ it.type }}</div>
        <template v-if="it.addedAt"><div class="dim">{{ t('Added') }}</div><div>{{ fmtAt(it.addedAt) }}</div></template>
        <template v-if="it.updatedAt"><div class="dim">{{ t('Last updated') }}</div><div>{{ fmtAt(it.updatedAt) }}</div></template>
        <template v-if="it.Media && it.Media[0] && it.Media[0].file"><div class="dim">{{ t('Path') }}</div><div style="word-break: break-all">{{ it.Media[0].file }}</div></template>
      </div>
      <p v-if="note" class="dim">{{ note }}</p>
    </template>
    <p v-if="error" class="err">{{ error }}</p>
    <template #actions>
      <button class="primary" :disabled="!it || busy" @click="save">{{ t('Save') }}</button>
      <button @click="emit('close')">{{ t('Cancel') }}</button>
    </template>
  </Modal>
</template>
