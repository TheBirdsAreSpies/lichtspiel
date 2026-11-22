<script setup lang="ts">
// A plugin's settings as a form: one field per setting, chosen by its value (as in the classic app). A tick
// box for true/false, a number box, a text box (hidden for passwords, tokens and keys), one line per entry
// for a list, a table for a list of flat objects (its columns also come from the plugin's defaults, so an
// emptied list keeps them); anything else nested stays JSON. "Edit as JSON" shows it all as JSON.
// `choices` (the plugin's setting_choices) offers values for a text box, by name or "list.field".
// The parent reads the values with the exposed value() (it throws on a wrong number or bad JSON).
import { ref } from 'vue';
import { t } from '../i18n';

type Scalar = string | number | boolean;
type Flat = Record<string, Scalar>;
interface Field {
  k: string, kind: 'table' | 'bool' | 'number' | 'text' | 'list' | 'json', secret?: boolean, numbers?: boolean,
  value: unknown, text: string, cols: string[], types: Record<string, string>, rows: Record<string, Scalar>[], choices?: string,
}
const props = defineProps<{ settings: Record<string, unknown>, defaults?: Record<string, unknown>, choices?: Record<string, unknown> }>();

let uid = 0;
const label = (k: string) => k.replace(/_/g, ' ').replace(/^./, c => c.toUpperCase());
const flat = (x: unknown): x is Flat => !!x && typeof x === 'object' && !Array.isArray(x) && Object.values(x as object).every(y => ['string', 'number', 'boolean'].includes(typeof y));
// The offered values of a text box: strings, or {value, label}.
const lists = ref<Record<string, { id: string, opts: { value: string, label: string }[] }>>({});
function choiceList(key: string) {
  const l = (props.choices || {})[key];
  if (!Array.isArray(l) || !l.length) return undefined;
  const id = 'pc' + (++uid);
  lists.value[id] = { id, opts: l.map(c => (typeof c === 'object' && c ? { value: String((c as { value: unknown }).value), label: String((c as { label?: string }).label || '') } : { value: String(c), label: '' })) };
  return id;
}
function makeField(k: string, v: unknown): Field {
  const def = (props.defaults || {})[k];
  const f: Field = { k, kind: 'json', value: v, text: '', cols: [], types: {}, rows: [] };
  const sample = Array.isArray(v) && v.length ? v : Array.isArray(def) ? def : [];
  if (Array.isArray(v) && sample.length && sample.every(flat) && v.every(flat)) {
    f.kind = 'table';
    f.cols = [...new Set([...sample, ...v].flatMap(o => Object.keys(o as Flat)))];
    for (const c of f.cols) {
      const x = [...v, ...sample].find(o => (o as Flat)[c] !== undefined) as Flat | undefined;
      f.types[c] = x ? typeof x[c] : 'string';
      const id = choiceList(k + '.' + c); if (id) f.types[c + ':list'] = id;
    }
    f.rows = (v as Flat[]).map(o => ({ ...o }));
  } else if (typeof v === 'boolean') f.kind = 'bool';
  else if (typeof v === 'number') { f.kind = 'number'; f.text = String(v); }
  else if (typeof v === 'string') { f.kind = 'text'; f.text = v; f.secret = /pass|secret|token|key/i.test(k); f.choices = choiceList(k); }
  else if (Array.isArray(v) && v.every(x => typeof x === 'string' || typeof x === 'number')) {
    f.kind = 'list'; f.text = v.join('\n'); f.numbers = v.length > 0 && v.every(x => typeof x === 'number');
  } else f.text = JSON.stringify(v, null, 2);
  return f;
}
const fields = ref<Field[]>([]);
const draw = (values: Record<string, unknown>) => { lists.value = {}; fields.value = Object.entries(values || {}).map(([k, v]) => makeField(k, v)); };
draw(props.settings);

function read(f: Field): unknown {
  const name = label(f.k);
  switch (f.kind) {
    case 'bool': return f.value;
    case 'number': { const n = Number(f.text); if (f.text.trim() === '' || !isFinite(n)) throw new Error(t('{name}: enter a number', { name })); return n; }
    case 'text': return f.text.trim();
    // A line starting with { or [ is read as JSON, so an empty list of rules can be filled too.
    case 'list': return f.text.split('\n').map(s => s.trim()).filter(Boolean).map(s => (/^[{[]/.test(s) ? JSON.parse(s) : f.numbers && isFinite(Number(s)) ? Number(s) : s));
    case 'table': return f.rows.map(r => {
      const o: Flat = {};
      for (const c of f.cols) {
        const ty = f.types[c], v = r[c];
        if (ty === 'boolean') o[c] = !!v;
        else if (ty === 'number') {
          if (v === undefined || String(v).trim() === '') continue;
          const n = Number(v); if (!isFinite(n)) throw new Error(t('{name}: enter a number', { name: label(c) })); o[c] = n;
        } else o[c] = String(v ?? '').trim();
      }
      return o;
    }).filter(o => f.cols.some(c => (typeof o[c] === 'string' ? o[c] !== '' : typeof o[c] === 'number')));
    default: try { return JSON.parse(f.text); } catch (e) { throw new Error(name + ': ' + (e as Error).message); }
  }
}
const fromForm = () => Object.fromEntries(fields.value.map(f => [f.k, read(f)]));

// As JSON: the values carry over each way; coming back from JSON the fields are drawn again from it.
const asJson = ref(!fields.value.length);
const json = ref(JSON.stringify(props.settings || {}, null, 2));
const error = ref('');
function toggle() {
  error.value = '';
  try {
    if (asJson.value) {
      const v = JSON.parse(json.value);
      if (!v || typeof v !== 'object' || Array.isArray(v)) throw new Error(t('Send the settings as a JSON object'));
      draw(v);
    } else json.value = JSON.stringify(fromForm(), null, 2);
    asJson.value = !asJson.value;
  } catch (e) { error.value = (e as Error).message; }
}
const value = () => (asJson.value ? JSON.parse(json.value) : fromForm());
defineExpose({ value, toggle, asJson });
const rowsOf = (f: Field) => Math.max(2, Math.min(8, f.text.split('\n').length + 1));
</script>

<template>
  <div>
    <div v-if="asJson" class="form">
      <label>{{ t('Settings') }}</label>
      <textarea v-model="json" rows="10" spellcheck="false" class="mono"></textarea>
    </div>
    <div v-else class="form">
      <template v-for="f in fields" :key="f.k">
        <label :for="'ps-' + f.k">{{ label(f.k) }}</label>
        <div v-if="f.kind === 'bool'" class="val"><input :id="'ps-' + f.k" v-model="f.value" type="checkbox"></div>
        <input v-else-if="f.kind === 'number'" :id="'ps-' + f.k" v-model="f.text" type="number" step="any">
        <input v-else-if="f.kind === 'text'" :id="'ps-' + f.k" v-model="f.text" :type="f.secret ? 'password' : 'text'" :autocomplete="f.secret ? 'new-password' : 'off'" spellcheck="false" :list="f.choices">
        <div v-else-if="f.kind === 'list'"><textarea :id="'ps-' + f.k" v-model="f.text" :rows="rowsOf(f)" spellcheck="false"></textarea><p class="dim hint">{{ t('One per line.') }}</p></div>
        <div v-else-if="f.kind === 'table'" class="table-wrap">
          <table class="table">
            <tr><th v-for="c in f.cols" :key="c">{{ label(c) }}</th><th></th></tr>
            <tr v-for="(r, i) in f.rows" :key="i">
              <td v-for="c in f.cols" :key="c">
                <input v-if="f.types[c] === 'boolean'" v-model="r[c]" type="checkbox" :aria-label="label(c)">
                <input v-else v-model="r[c]" :type="f.types[c] === 'number' ? 'number' : 'text'" step="any" spellcheck="false" :list="f.types[c + ':list']" :aria-label="label(c)">
              </td>
              <td><button class="small danger" :title="t('Remove')" @click="f.rows.splice(i, 1)">✕</button></td>
            </tr>
          </table>
          <button class="small" @click="f.rows.push({})">{{ t('+ Add entry') }}</button>
        </div>
        <div v-else><textarea :id="'ps-' + f.k" v-model="f.text" rows="4" spellcheck="false" class="mono"></textarea><p class="dim hint">{{ t('As JSON.') }}</p></div>
      </template>
    </div>
    <datalist v-for="l in lists" :id="l.id" :key="l.id"><option v-for="o in l.opts" :key="o.value" :value="o.value" :label="o.label"></option></datalist>
    <p v-if="error" class="err">{{ error }}</p>
  </div>
</template>
