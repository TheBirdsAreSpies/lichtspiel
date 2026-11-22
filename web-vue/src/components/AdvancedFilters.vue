<script setup lang="ts">
// "Advanced filters" of a library: all / any of a list of rules (field, operator, value), as in the classic
// app. Applied, they go into the address (?rules=[…]&rmatch=any) like the other filters.
import { reactive, ref } from 'vue';
import { filterName, ruleFields, type Rule, type RuleField } from '../filters';
import { t } from '../i18n';
import Modal from './Modal.vue';

const props = defineProps<{ rules: Rule[], any: boolean }>();
const emit = defineEmits<{ close: [], apply: [Rule[], boolean] }>();

// Operators in plain words, by the kind of field.
const OPS: Record<string, [string, string][]> = {
  text: [['contains', 'contains'], ['not_contains', 'does not contain'], ['is', 'is'], ['is_not', 'is not'], ['starts_with', 'begins with'], ['ends_with', 'ends with']],
  number: [['is', 'is'], ['is_not', 'is not'], ['gt', 'is greater than'], ['lt', 'is less than']],
  date: [['gt', 'is after'], ['lt', 'is before'], ['in_last_days', 'is within the last … days'], ['not_in_last_days', 'is not within the last … days']],
  bool: [['is', 'applies']],
};
const FIELDS = ['title', 'year', 'genre', 'content_rating', 'studio', 'summary', 'added_at', 'release_date', 'last_viewed_at', 'duration', 'audience_rating', 'critic_rating', 'user_rating',
  'view_count', 'progress', 'resolution', 'bitrate', 'size', 'container', 'video_codec', 'audio_codec', 'audio_language', 'subtitle_language', 'director', 'actor', 'writer', 'producer',
  'country', 'collection', 'label', 'edition', 'folder', 'unwatched', 'in_progress', 'unmatched', 'duplicate', 'hdr', 'dovi', 'favorite', 'watchlist'];
const NAMES: Record<string, string> = { title: 'Title', summary: 'Description', added_at: 'Date added', release_date: 'Release date', last_viewed_at: 'Date played', duration: 'Length (minutes)',
  audience_rating: 'Audience rating', critic_rating: 'Critic rating', user_rating: 'Your rating', view_count: 'Plays', progress: 'Progress (%)', resolution: 'Picture height (pixels)',
  bitrate: 'Bitrate (kbit/s)', size: 'File size (MB)', container: 'File type', favorite: 'Favorite', watchlist: 'On the watchlist', unwatched: 'Unwatched', unmatched: 'Unmatched' };
const fieldName = (f: string) => (NAMES[f] ? t(NAMES[f]) : filterName(f));

const known = ref<RuleField[]>([]);
ruleFields().then(f => { known.value = f.fields; }).catch(() => {});
const typeOf = (f: string) => { const d = known.value.find(x => x.field === f); const ty = d ? d.type : 'text'; return ty === 'sort' ? 'text' : ty; };
const opsOf = (r: Rule) => OPS[typeOf(r.field)] || OPS.text;

const rules = reactive<Rule[]>(props.rules.length ? JSON.parse(JSON.stringify(props.rules)) : [{ field: 'title', op: 'contains', value: '' }]);
const any = ref(props.any);
function setField(r: Rule, f: string) {
  r.field = f;
  const ty = typeOf(f);
  r.op = (OPS[ty] || OPS.text)[0][0];
  r.value = ty === 'bool' ? true : '';
}
const day = (v: unknown) => (v ? new Date(Number(v) * 1000).toISOString().slice(0, 10) : '');
const undays = (v: string) => (v ? Math.floor(new Date(v + 'T00:00:00Z').getTime() / 1000) : '');
function apply() {
  const ok = rules.filter(r => r.value !== '' && r.value !== undefined && r.value !== null).map(r => {
    // An operator that does not fit the field (it changed type) becomes the first that does.
    const op = opsOf(r).some(o => o[0] === r.op) ? r.op : opsOf(r)[0][0];
    return { field: r.field, op, value: r.value };
  });
  emit('apply', ok, any.value && ok.length > 0);
}
</script>

<template>
  <Modal :title="t('Advanced filters')" wide @close="emit('close')">
    <div class="form">
      <label for="af-match">{{ t('Show items that match') }}</label>
      <select id="af-match" :value="any ? 'any' : 'all'" @change="any = ($event.target as HTMLSelectElement).value === 'any'">
        <option value="all">{{ t('all of the following') }}</option><option value="any">{{ t('any of the following') }}</option>
      </select>
    </div>
    <div class="rules">
      <div v-for="(r, i) in rules" :key="i" class="rule">
        <select :value="r.field" @change="setField(r, ($event.target as HTMLSelectElement).value)">
          <option v-for="f in FIELDS" :key="f" :value="f">{{ fieldName(f) }}</option>
        </select>
        <select v-if="typeOf(r.field) !== 'bool'" :value="opsOf(r).some(o => o[0] === r.op) ? r.op : opsOf(r)[0][0]" @change="r.op = ($event.target as HTMLSelectElement).value; r.value = ''">
          <option v-for="[o, l] in opsOf(r)" :key="o" :value="o">{{ t(l) }}</option>
        </select>
        <select v-if="typeOf(r.field) === 'bool'" :value="r.value === false ? 'false' : 'true'" @change="r.value = ($event.target as HTMLSelectElement).value === 'true'">
          <option value="true">{{ t('yes') }}</option><option value="false">{{ t('no') }}</option>
        </select>
        <input v-else-if="typeOf(r.field) === 'date' && !r.op.includes('days')" type="date" :value="day(r.value)" @change="r.value = undays(($event.target as HTMLInputElement).value)">
        <input v-else :value="r.value ?? ''" :type="typeOf(r.field) === 'number' || r.op.includes('days') ? 'number' : 'text'"
          :placeholder="r.op.includes('days') ? t('days') : t('value')" @change="r.value = ($event.target as HTMLInputElement).value">
        <button class="danger" :title="t('Remove')" :aria-label="t('Remove')" @click="rules.splice(i, 1)">✕</button>
      </div>
      <button @click="rules.push({ field: 'genre', op: 'is', value: '' })">{{ t('+ Add rule') }}</button>
    </div>
    <template #actions>
      <button class="primary" @click="apply">{{ t('Apply') }}</button>
      <button v-if="props.rules.length" @click="emit('apply', [], false)">{{ t('Remove all') }}</button>
      <button @click="emit('close')">{{ t('Cancel') }}</button>
    </template>
  </Modal>
</template>
