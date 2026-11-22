<script setup lang="ts">
// A smart playlist's rules (new, or "Edit rules"): all / any of a list of rules (field, operator, value),
// the order, a limit and the libraries it looks in. Below, how many titles match now (POST /playlists/preview).
// The fields and operators come from the server (GET /api/filters), so new ones show up here by themselves.
import { computed, reactive, ref, watch } from 'vue';
import { useRouter } from 'vue-router';
import { api, list, session, type Item, type Playlist } from '../api';
import { ruleFields, type Rule, type RuleField } from '../filters';
import { plTitle } from '../format';
import { t } from '../i18n';
import Modal from './Modal.vue';

interface Query { match_all: boolean, rules: Rule[], sort: string, limit: number | null, sections: number[] }
const props = defineProps<{ pl?: Playlist | null }>();
const emit = defineEmits<{ close: [], saved: [] }>();
const router = useRouter();

// Which operators fit which kind of field.
const OPS: Record<string, string[]> = { bool: ['is', 'is_not'], date: ['in_last_days', 'not_in_last_days', 'gt', 'lt'], number: ['is', 'is_not', 'gt', 'gte', 'lt', 'lte', 'between', 'in'],
  text: ['is', 'is_not', 'contains', 'not_contains', 'starts_with', 'ends_with', 'in'] };
const SORTS: [string, string][] = [['title', 'Title'], ['year:desc', 'Year (newest)'], ['year', 'Year (oldest)'], ['added_at:desc', 'Recently added'], ['rating:desc', 'Rating'],
  ['duration:desc', 'Length'], ['last_viewed_at:desc', 'Recently watched'], ['view_count:desc', 'Most watched'], ['random', 'Random'], ['episode_order', 'Show, season, episode']];

const fields = ref<RuleField[]>([]);
const operators = ref<{ op: string, label: string }[]>([]);
const q = reactive<Query>(props.pl && props.pl.query ? JSON.parse(JSON.stringify(props.pl.query))
  : { match_all: true, rules: [{ field: 'kind', op: 'is', value: 'movie' }], sort: 'title', limit: null, sections: [] });
if (!q.sections) q.sections = [];
const name = ref(props.pl ? plTitle(props.pl) : '');
const shared = ref(!!(props.pl && props.pl.shared));
const pinned = ref(!!(props.pl && props.pl.pinned));
const preview = ref('');
const error = ref('');

ruleFields().then(f => { fields.value = f.fields.filter(x => x.type !== 'sort'); operators.value = f.operators; }).catch(e => { error.value = (e as Error).message; });

const fieldOf = (r: Rule) => fields.value.find(x => x.field === r.field) || fields.value[0];
const opsFor = (type?: string) => operators.value.filter(o => (OPS[type || 'text'] || OPS.text).includes(o.op));
function setField(r: Rule, f: string) {
  r.field = f;
  const nf = fields.value.find(x => x.field === f);
  r.op = (opsFor(nf && nf.type)[0] || { op: 'is' }).op;
  r.value = nf && nf.type === 'bool' ? true : '';
}
const shownValue = (r: Rule) => (Array.isArray(r.value) ? r.value.join(',') : String(r.value ?? ''));
function setValue(r: Rule, v: string) {
  const f = fieldOf(r);
  r.value = r.op === 'between' ? v.split(',').map(Number) : (f && f.type === 'number' || r.op.includes('days')) && v !== '' && !isNaN(Number(v)) ? Number(v) : v;
}
function toggleSection(id: number, on: boolean) {
  q.sections = q.sections.filter(x => x !== id);
  if (on) q.sections.push(id);
}

// How many match, a moment after each change.
let timer = 0;
watch(() => JSON.stringify(q), () => {
  clearTimeout(timer);
  timer = window.setTimeout(async () => {
    try {
      const r = await api<{ MediaContainer: { totalSize: number, Metadata?: Item[] } }>('POST', '/playlists/preview', { query: q });
      const names = (r.MediaContainer.Metadata || []).slice(0, 6).map(m => (m.type === 'episode' ? `${m.grandparentTitle} ${t('S{n}', { n: m.parentIndex || 0 })}${t('E{n}', { n: m.index || 0 })}` : m.type === 'track' ? `${m.grandparentTitle} · ${m.title}` : m.title));
      preview.value = t('{n} items match: ', { n: r.MediaContainer.totalSize }) + names.join(', ') + (r.MediaContainer.totalSize > 6 ? ' …' : '');
    } catch (e) { preview.value = (e as Error).message; }
  }, 250);
}, { immediate: true });

const busy = ref(false);
const isDefault = computed(() => !!(props.pl && props.pl.default));
async function save() {
  error.value = '';
  if (!name.value.trim()) { error.value = t('Give it a name'); return; }
  // A default playlist keeps its English name unless it was really renamed.
  const title = props.pl && props.pl.default && name.value === plTitle(props.pl) ? props.pl.title : name.value.trim();
  busy.value = true;
  try {
    const body = { title, query: q, shared: shared.value, pinned: pinned.value };
    if (props.pl) { await api('PUT', '/playlists/' + props.pl.id, body); emit('saved'); emit('close'); }
    else {
      const r = await api('POST', '/playlists', { ...body, kind: 'smart' });
      const made = list<Playlist>(r)[0];
      emit('close');
      if (made) router.push('/playlist/' + made.id);
    }
  } catch (e) { error.value = (e as Error).message; busy.value = false; }
}
</script>

<template>
  <Modal :title="pl ? t('Edit smart playlist') : t('New smart playlist')" wide @close="emit('close')">
    <div class="form">
      <label for="sp-name">{{ t('Name') }}</label>
      <input id="sp-name" v-model="name" :placeholder="t('e.g. Unwatched sci-fi from the 90s')">
      <label for="sp-match">{{ t('Include items that match') }}</label>
      <select id="sp-match" :value="q.match_all ? 'all' : 'any'" @change="q.match_all = ($event.target as HTMLSelectElement).value === 'all'">
        <option value="all">{{ t('all of these rules') }}</option><option value="any">{{ t('any of these rules') }}</option>
      </select>
    </div>
    <div class="rules">
      <div v-for="(r, i) in q.rules" :key="i" class="rule">
        <select :value="r.field" :aria-label="t('Field')" @change="setField(r, ($event.target as HTMLSelectElement).value)">
          <option v-for="f in fields" :key="f.field" :value="f.field">{{ t(f.label) }}</option>
        </select>
        <select v-model="r.op" :aria-label="t('Operator')">
          <option v-for="o in opsFor(fieldOf(r)?.type)" :key="o.op" :value="o.op">{{ t(o.label) }}</option>
        </select>
        <select v-if="fieldOf(r)?.type === 'bool'" :value="r.value === false ? 'false' : 'true'" :aria-label="t('value')" @change="r.value = ($event.target as HTMLSelectElement).value === 'true'">
          <option value="true">{{ t('yes') }}</option><option value="false">{{ t('no') }}</option>
        </select>
        <input v-else :value="shownValue(r)" :aria-label="t('value')" :placeholder="r.op === 'between' ? t('low,high') : r.op.includes('days') ? t('days') : t('value')"
          @change="setValue(r, ($event.target as HTMLInputElement).value)">
        <button class="danger" :title="t('Remove')" :aria-label="t('Remove')" @click="q.rules.splice(i, 1)">✕</button>
      </div>
      <button @click="q.rules.push({ field: 'genre', op: 'is', value: '' })">{{ t('+ Add rule') }}</button>
    </div>
    <div class="form">
      <label for="sp-sort">{{ t('Sort by') }}</label>
      <select id="sp-sort" v-model="q.sort"><option v-for="[v, l] in SORTS" :key="v" :value="v">{{ t(l) }}</option></select>
      <label for="sp-limit">{{ t('Limit (empty = no limit)') }}</label>
      <input id="sp-limit" type="number" min="1" :value="q.limit ?? ''" @change="q.limit = ($event.target as HTMLInputElement).value ? Number(($event.target as HTMLInputElement).value) : null">
      <label>{{ t('Libraries') }}</label>
      <div class="checks">
        <label v-for="s in session.sections" :key="s.id"><input type="checkbox" :checked="q.sections.includes(s.id)" @change="toggleSection(s.id, ($event.target as HTMLInputElement).checked)"> {{ s.title }}</label>
        <span class="dim">{{ t('(none ticked = all)') }}</span>
      </div>
    </div>
    <div v-if="!isDefault" class="checks" style="margin-top: 12px">
      <label><input v-model="shared" type="checkbox"> {{ t('Share with everyone') }}</label>
      <label><input v-model="pinned" type="checkbox"> {{ t('Show on home') }}</label>
    </div>
    <p class="dim">{{ preview }}</p>
    <p v-if="error" class="err">{{ error }}</p>
    <template #actions>
      <button class="primary" :disabled="busy" @click="save">{{ t('Save') }}</button>
      <button @click="emit('close')">{{ t('Cancel') }}</button>
    </template>
  </Modal>
</template>
