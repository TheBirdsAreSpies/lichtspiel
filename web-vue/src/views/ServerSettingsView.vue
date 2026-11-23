<script setup lang="ts">
// The server's settings (owner), one tab each (/server/<tab>), drawn from the rows in serverSettings.ts;
// a search goes through every tab. Save sends them all (PUT /api/settings). Maintenance and the API (tokens,
// every address) have their own panels. The page stays while the tabs change (App.vue keys it by name).
import { computed, ref } from 'vue';
import { api, get, session } from '../api';
import { lang, t } from '../i18n';
import { NEEDS_FFMPEG, ROWS, TABS, type Opt, type Row, type SettingsReply, type Transcoder } from '../serverSettings';
import { toast } from '../toast';
import AskDialog from '../components/AskDialog.vue';
import MaintenancePanel from '../components/MaintenancePanel.vue';
import ApiView from './ApiView.vue';

const props = defineProps<{ tab?: string }>();
const tab = computed(() => (TABS.some(([k]) => k === props.tab) ? props.tab! : 'general'));
const r = ref<SettingsReply | null>(null);
const tc = ref<Transcoder | null>(null);
const vals = ref<Record<string, unknown>>({});
const error = ref('');
async function load() {
  try {
    const [a, b] = await Promise.all([get<SettingsReply>('/api/settings'), get<Transcoder>('/api/transcoder')]);
    r.value = a; tc.value = b; vals.value = { ...a.settings };
  } catch (e) { error.value = (e as Error).message; }
}
load();

const query = ref('');
type Field = Exclude<Row, { kind: 'head' | 'note' | 'info' | 'button' }>;
const isField = (x: Row): x is Field => 'k' in x;
const text = (x: Row) => [x.kind !== 'info' ? t(x.label) : '', 'help' in x && x.help ? t(x.help) : ''].join(' ').toLowerCase();
// What shows: the tab's rows, or every row that matches the search (by tab).
const shown = computed<[string, Row[]][]>(() => {
  const q = query.value.trim().toLowerCase();
  if (!q) return [[tab.value, ROWS[tab.value] || []]];
  return TABS.filter(([k]) => ROWS[k]).map(([k]) => [k, ROWS[k].filter(x => x.kind !== 'info' && text(x).includes(q))] as [string, Row[]]).filter(([, rows]) => rows.length);
});
const tabTitle = (k: string) => t((TABS.find(x => x[0] === k) || [k, k])[1]);

const opts = (x: Field): Opt[] => (x.kind === 'select' ? (typeof x.opts === 'function' ? (r.value && tc.value ? x.opts(r.value, tc.value) : []) : x.opts) : []);
const help = (x: Row) => ('help' in x && x.help ? t(x.help, 'params' in x && x.params && r.value ? x.params(r.value) : undefined) : '');
const off = (k: string) => !!tc.value && !tc.value.ffmpeg_found && NEEDS_FFMPEG.includes(k);
// No graphics device found: a field to type one into.
const typedDevice = (x: Field) => x.k === 'hw_device' && !(tc.value && (tc.value.devices || []).length);
function setVal(k: string, v: string | boolean) {
  const old = r.value && r.value.settings[k];
  vals.value[k] = typeof v === 'boolean' ? v : typeof old === 'number' ? Number(v) : v;
}
const str = (k: string) => { const v = vals.value[k]; return v === null || v === undefined ? '' : String(v); };

const saving = ref(false);
async function save() {
  if (!r.value) return;
  saving.value = true;
  // The time zone on every save, so a change of clocks is picked up.
  const body: Record<string, unknown> = { utc_offset_minutes: -new Date().getTimezoneOffset() };
  for (const rows of Object.values(ROWS)) for (const x of rows) if (isField(x)) body[x.k] = vals.value[x.k];
  try {
    const x = await api<{ refetching?: number, restart_needed?: string }>('PUT', '/api/settings', body);
    toast([t('Saved'), x.refetching ? t('details for {n} movies and shows are being downloaded again in the new language', { n: x.refetching }) : '', x.restart_needed ? t(x.restart_needed) : ''].filter(Boolean).join(' · '), 6000);
    // "Forgot password" on the sign-in page follows the mail server.
    get<typeof session.status>('/api/status').then(s => { session.status = s; }).catch(() => {});
    load();
  } catch (e) { toast((e as Error).message, 5000); } finally { saving.value = false; }
}

// Saves the mail settings, then sends a test mail.
const mailAsk = ref(false);
async function testMail(to: string | null) {
  mailAsk.value = false;
  if (to === null) return;
  const body: Record<string, unknown> = {};
  for (const k of ['smtp_host', 'smtp_port', 'smtp_security', 'smtp_user', 'smtp_password', 'smtp_from']) body[k] = vals.value[k];
  try {
    await api('PUT', '/api/settings', body);
    const x = await api<{ to: string }>('POST', '/api/settings/mail-test', { to, lang: lang.value });
    toast(t('Test mail sent to {to}', { to: x.to }), 5000);
  } catch (e) { toast((e as Error).message, 9000); }
}
async function imdb() {
  try { await api('POST', '/api/imdb-ratings/refresh'); toast(t('Downloading IMDb ratings — progress is shown at the top'), 5000); } catch (e) { toast((e as Error).message, 6000); }
}
const where = computed(() => {
  const x = r.value; if (!x || !x.your_address) return '';
  return t('The server sees you as {ip} — {where}.', { ip: x.your_address, where: !x.your_address_trusted ? t('passed on by a proxy that isn’t trusted') : x.you_are_local ? t('home network') : t('from outside') });
});
</script>

<template>
  <div class="settings">
    <div class="dash-head">
      <h1>{{ t('Server settings') }}</h1>
      <input v-model="query" type="search" class="set-search" :placeholder="t('Search settings')" :aria-label="t('Search settings')">
    </div>
    <nav class="tabs flat">
      <RouterLink v-for="[k, l] in TABS" :key="k" :to="'/server/' + k" custom v-slot="{ navigate }">
        <button :class="{ on: tab === k && !query.trim() }" @click="query = ''; navigate()">{{ t(l) }}</button>
      </RouterLink>
    </nav>
    <p v-if="error" class="err">{{ error }}</p>

    <MaintenancePanel v-if="tab === 'maintenance' && !query.trim()" />
    <ApiView v-else-if="tab === 'api' && !query.trim()" embedded />
    <template v-else-if="r && tc">
      <p v-if="!shown.length" class="empty">{{ t('No setting matches “{q}”.', { q: query.trim() }) }}</p>
      <section v-for="[k, rows] in shown" :key="k" class="panel">
        <h2><RouterLink v-if="query.trim()" :to="'/server/' + k" @click="query = ''">{{ tabTitle(k) }}</RouterLink><template v-else>{{ tabTitle(k) }}</template></h2>
        <div class="form">
          <template v-for="(x, i) in rows" :key="i">
            <h3 v-if="x.kind === 'head'" class="form-wide">{{ t(x.label) }}</h3>
            <p v-else-if="x.kind === 'note'" class="form-wide dim">{{ t(x.label) }}</p>
            <template v-else-if="x.kind === 'info' && x.what === 'lan'">
              <label>{{ t('Address in the home network') }}</label>
              <div class="val"><b>{{ r.lan_url }}</b><p class="dim hint">{{ t('Where devices at home reach the server. For access from outside, forward this port on your router and enter the address people use below.') }}</p></div>
            </template>
            <p v-else-if="x.kind === 'info' && x.what === 'you'" class="form-wide"><b>{{ where }}</b></p>
            <div v-else-if="x.kind === 'info' && x.what === 'transcoder'" class="form-wide">
              <p :class="tc.ffmpeg_found ? 'ok' : 'err'">{{ tc.ffmpeg_found ? t('ffmpeg found: {version}', { version: tc.ffmpeg_version || '' }) : t('ffmpeg was not found. Install it (the Docker image includes it) or enter its path below; without it only browser-friendly files play.') }}</p>
              <template v-if="tc.hardware_encoders.length">
                <p class="dim">{{ t('Hardware encoders in this ffmpeg: ') + tc.hardware_encoders.join(', ') + (tc.render_device ? ' · ' + t('/dev/dri/renderD128 present') : '') }}</p>
                <p :class="(tc.hardware_working || []).length ? 'ok' : 'dim'">{{ (tc.hardware_working || []).length ? t('Working on this computer: ') + tc.hardware_working!.join(', ') : t('None of them works on this computer, so the processor converts.') }}</p>
              </template>
              <p class="dim">{{ t('Converting with: ') + (tc.hw_in_use === 'none' ? t('the processor (software)') : tc.hw_in_use) }}</p>
            </div>
            <template v-else-if="x.kind === 'button'">
              <span></span>
              <div class="val">
                <button class="small" @click="x.what === 'mail' ? (mailAsk = true) : imdb()">{{ t(x.label) }}</button>
                <p v-if="x.help" class="dim hint">{{ t(x.help) }}</p>
              </div>
            </template>
            <template v-else-if="isField(x) && x.kind === 'check'">
              <span></span>
              <div class="val" :class="{ faded: off(x.k) }" :title="off(x.k) ? t('Needs ffmpeg') : undefined">
                <label class="check"><input type="checkbox" :checked="!!vals[x.k]" :disabled="off(x.k)" @change="setVal(x.k, ($event.target as HTMLInputElement).checked)"> {{ t(x.label) }}</label>
                <p v-if="help(x)" class="dim hint">{{ help(x) }}</p>
              </div>
            </template>
            <template v-else-if="isField(x)">
              <label :for="'s-' + x.k" :class="{ faded: off(x.k) }">{{ t(x.label) }}</label>
              <div class="val" :class="{ faded: off(x.k) }" :title="off(x.k) ? t('Needs ffmpeg') : undefined">
                <select v-if="x.kind === 'select' && !typedDevice(x)" :id="'s-' + x.k" :value="str(x.k)" :disabled="off(x.k)" @change="setVal(x.k, ($event.target as HTMLSelectElement).value)">
                  <option v-for="o in opts(x)" :key="String(o[0])" :value="String(o[0])">{{ t(o[1], o[2]) }}</option>
                </select>
                <textarea v-else-if="x.kind === 'area'" :id="'s-' + x.k" rows="4" :value="str(x.k)" placeholder="D:\Videos\intro.mp4" @input="setVal(x.k, ($event.target as HTMLTextAreaElement).value)"></textarea>
                <input v-else :id="'s-' + x.k" :type="x.kind === 'select' ? 'text' : x.kind" :value="str(x.k)" :disabled="off(x.k)" @input="setVal(x.k, ($event.target as HTMLInputElement).value)">
                <p v-if="typedDevice(x)" class="dim hint">{{ t('Empty = the first one. VAAPI: the device file, such as /dev/dri/renderD129. NVIDIA: the card number, such as 1.') }}</p>
                <p v-else-if="help(x)" class="dim hint">{{ help(x) }}</p>
              </div>
            </template>
          </template>
        </div>
      </section>
      <div v-if="shown.length" class="actions"><button class="primary" :disabled="saving" @click="save">{{ t('Save settings') }}</button></div>
      <!-- (Not in the Docker image: there it is always /config inside the container, not the folder on the disk.) -->
      <p v-if="tab === 'general' && !query.trim() && r.data_dir !== '/config'" class="dim">{{ t('Data folder on the server: ') }}<code>{{ r.data_dir }}</code></p>
    </template>
    <div v-else-if="!error" class="skel line"></div>
    <AskDialog v-if="mailAsk" :title="t('Send the test mail to')" :value="session.me?.email || ''" :ok="t('Send')" @done="testMail" />
  </div>
</template>
