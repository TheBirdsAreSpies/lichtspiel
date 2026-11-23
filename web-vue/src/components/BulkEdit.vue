<script setup lang="ts">
// Several titles' details in one box (owner), as in the classic app: only what is changed is saved. A field
// that differs between the titles says "Mixed"; a tag list shows what all have in common, and what is added
// or taken away there is added to or taken from each title (the rest of each one's tags stays).
import { onMounted, ref } from 'vue';
import { api, get, list, type Item } from '../api';
import { t } from '../i18n';
import { toast } from '../toast';
import Modal from './Modal.vue';

const props = defineProps<{ ids: number[] }>();
const emit = defineEmits<{ close: [], saved: [] }>();
type Any = Record<string, any>; // eslint-disable-line @typescript-eslint/no-explicit-any
const its = ref<Any[] | null>(null);
const langs = ref<{ code: string, name: string }[]>([]);
const tab = ref(0);
const saving = ref(false);
const ex = (it: Any) => (it.extra || {}) as Any;

interface Field { key: string, label: string, kind: 'text' | 'area' | 'date' | 'num' | 'tags' | 'choice', get: (it: Any) => unknown, opts?: () => [string, string][], value: string, init: string, mixed: boolean, common: string[] }
const field = (key: string, label: string, kind: Field['kind'], get: (it: Any) => unknown, opts?: () => [string, string][]): Field =>
  ({ key, label, kind, get, opts, value: '', init: '', mixed: false, common: [] });
const lib = (): [string, string][] => [['', t('Library default')]];
const PANES: [string, Field[]][] = [
  ['General', [
    field('original_title', 'Original title', 'text', it => ex(it).original_title),
    field('edition', 'Edition (e.g. Director’s Cut)', 'text', it => ex(it).edition),
    field('originally_available', 'Originally available', 'date', it => String(ex(it).originally_available || '').slice(0, 10)),
    field('year', 'Year', 'num', it => it.year), field('content_rating', 'Age rating', 'text', it => it.contentRating),
    field('rating', 'Rating (0-10)', 'num', it => it.rating), field('studio', 'Studio', 'text', it => it.studio),
    field('tagline', 'Tagline', 'text', it => it.tagline), field('summary', 'Summary', 'area', it => it.summary)]],
  ['Tags', [
    field('directors', 'Directors', 'tags', it => ex(it).directors), field('countries', 'Countries', 'tags', it => ex(it).countries),
    field('genres', 'Genres', 'tags', it => it.genres), field('writers', 'Writers', 'tags', it => ex(it).writers),
    field('producers', 'Producers', 'tags', it => ex(it).producers), field('collections', 'Collections', 'tags', it => ex(it).collections)]],
  ['Labels', [field('labels', 'Labels (comma separated)', 'tags', it => ex(it).labels)]],
  ['Advanced', [
    field('language_override', 'Language for details', 'choice', it => ex(it).language_override, () => [...lib(), ...langs.value.map(l => [l.code, l.name] as [string, string])]),
    field('credits_detection', 'Detect credits', 'choice', it => ex(it).credits_detection, () => [...lib(), ['on', t('Yes')], ['off', t('No')]]),
    field('use_original_title', 'Use original title', 'choice', it => ex(it).use_original_title, () => [...lib(), ['yes', t('Yes')], ['no', t('No')]])]],
];
const fields = ref(PANES);
const MIXED = '__mixed';
const split = (s: string) => s.split(',').map(x => x.trim()).filter(Boolean);
const tagsOf = (f: Field, it: Any) => ((f.get(it) as string[] | undefined) || []);

onMounted(async () => {
  try {
    const [all, st] = await Promise.all([
      Promise.all(props.ids.map(id => get('/library/metadata/' + id).then(j => list<Item>(j)[0] as Any))),
      get<{ languages?: { code: string, name: string }[] }>('/api/settings').catch(() => ({ languages: [] })),
    ]);
    langs.value = st.languages || [];
    const movies = all.every(it => it.type === 'movie' || it.type === 'episode');
    for (const [, fs] of fields.value) for (const f of fs) {
      if (f.kind === 'tags') {
        const lists = all.map(it => tagsOf(f, it));
        f.common = lists[0].filter(v => lists.every(l => l.includes(v)));
        f.mixed = lists.some(l => l.length !== f.common.length);
        f.value = f.init = f.common.join(', ');
      } else {
        const vals = all.map(it => { const v = f.get(it); return v == null ? '' : String(v); });
        f.mixed = vals.some(v => v !== vals[0]);
        f.init = f.mixed ? (f.kind === 'choice' ? MIXED : '') : vals[0];
        f.value = f.init;
      }
    }
    if (!movies) fields.value[0][1] = fields.value[0][1].filter(f => f.key !== 'edition');
    its.value = all;
  } catch (e) { toast((e as Error).message, 6000); emit('close'); }
});

// What to send to each title: only the fields that were changed.
function bodyFor(it: Any) {
  const body: Any = {};
  for (const [, fs] of fields.value) for (const f of fs) {
    if (f.kind === 'tags') {
      const now = split(f.value), add = now.filter(v => !f.common.includes(v)), del = f.common.filter(v => !now.includes(v));
      if (!add.length && !del.length) continue;
      const own = tagsOf(f, it);
      body[f.key] = [...own.filter(v => !del.includes(v)), ...add.filter(v => !own.includes(v))];
    } else if (f.value !== f.init && f.value !== MIXED) {
      body[f.key] = f.kind === 'num' ? (f.value === '' ? null : Number(String(f.value).replace(',', '.'))) : f.value;
    }
  }
  return body;
}
async function save() {
  const jobs = (its.value || []).map(it => [it, bodyFor(it)] as const).filter(([, b]) => Object.keys(b).length);
  if (!jobs.length) { toast(t('Nothing changed')); emit('close'); return; }
  saving.value = true;
  try { for (const [it, b] of jobs) await api('PUT', '/library/metadata/' + it.id, b); }
  catch (e) { saving.value = false; toast((e as Error).message, 6000); return; }
  toast(t('{n} titles saved', { n: jobs.length })); emit('saved'); emit('close');
}
</script>

<template>
  <Modal :title="t('Edit {n} titles', { n: ids.length })" wide @close="emit('close')">
    <p class="dim">{{ t('Only what you change is saved. A field that differs between the titles says “Mixed”.') }}</p>
    <div v-if="!its" class="skel line"></div>
    <template v-else>
      <div class="tabs flat">
        <button v-for="([name], i) in fields" :key="name" :class="{ on: tab === i }" @click="tab = i">{{ t(name) }}</button>
      </div>
      <div class="form">
        <template v-for="f in fields[tab][1]" :key="f.key">
          <label :for="'b-' + f.key">{{ t(f.label) }}</label>
          <div>
            <textarea v-if="f.kind === 'area'" :id="'b-' + f.key" v-model="f.value" rows="4" :placeholder="f.mixed ? t('Mixed') : ''"></textarea>
            <select v-else-if="f.kind === 'choice'" :id="'b-' + f.key" v-model="f.value">
              <option v-if="f.mixed" :value="MIXED">{{ t('Mixed') }}</option>
              <option v-for="[v, n] in f.opts!()" :key="v" :value="v">{{ n }}</option>
            </select>
            <input v-else :id="'b-' + f.key" v-model="f.value" :type="f.kind === 'date' ? 'date' : 'text'" :inputmode="f.kind === 'num' ? 'decimal' : undefined" :placeholder="f.mixed ? t('Mixed') : ''">
            <p v-if="f.kind === 'tags' && f.mixed" class="dim hint">{{ t('What only some of the titles have stays as it is. Add it here to give it to all.') }}</p>
          </div>
        </template>
      </div>
    </template>
    <template #actions>
      <button class="primary" :disabled="!its || saving" @click="save">{{ t('Save') }}</button>
      <button @click="emit('close')">{{ t('Cancel') }}</button>
    </template>
  </Modal>
</template>
