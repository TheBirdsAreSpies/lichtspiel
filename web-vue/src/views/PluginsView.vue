<script setup lang="ts">
// Plugins (owner): each installed plugin with its switch, where its page shows, and its settings;
// reload the plugin folder, upload a plugin as a .zip; what the plugins did lately and the events they hear.
import { onMounted, ref, shallowReactive } from 'vue';
import { api, ApiError, get } from '../api';
import { lang, t } from '../i18n';
import Modal from '../components/Modal.vue';
import PluginSettings from '../components/PluginSettings.vue';
import { loadPanels } from '../plugins';

interface Plugin {
  id: string, name: string, version?: string, author?: string, description?: string, enabled: boolean, hooks: string[],
  settings: Record<string, unknown>, defaults?: Record<string, unknown>, setting_choices?: Record<string, unknown>,
  user_panel?: boolean, placement?: string, suggested_placement?: string,
}
interface LogLine { at: number, plugin: string, event: string, ok: boolean, detail?: string }

const plugins = ref<Plugin[] | null>(null);
const folder = ref('');
const errors = ref<string[]>([]);
const events = ref<string[]>([]);
const log = ref<LogLine[]>([]);
const error = ref('');
const note = ref('');
let noteTimer = 0;
function say(text: string) { note.value = text; clearTimeout(noteTimer); noteTimer = window.setTimeout(() => { note.value = ''; }, 5000); }
// Each plugin's form (not made reactive itself: they are components).
const forms = shallowReactive<Record<string, InstanceType<typeof PluginSettings>>>({});
const rev = ref(0); // drawn again (forms too) after a change

async function load() {
  try {
    const [r, l] = await Promise.all([get<{ plugins: Plugin[], folder: string, errors: string[], events: string[] }>('/api/plugins'), get<{ log: LogLine[] }>('/api/plugins/log')]);
    plugins.value = r.plugins; folder.value = r.folder; errors.value = r.errors; events.value = r.events; log.value = l.log.slice(0, 40);
    rev.value++;
  } catch (e) { error.value = (e as Error).message; }
}
onMounted(load);

async function reload() {
  try { const x = await api<{ count: number }>('POST', '/api/plugins/reload'); say(t('{n} plugin(s) found', { n: x.count })); load(); } catch (e) { say((e as Error).message); }
}
async function toggle(p: Plugin) {
  try { await api('POST', `/api/plugins/${p.id}/${p.enabled ? 'disable' : 'enable'}`); load(); loadPanels(); } catch (e) { say((e as Error).message); }
}
async function place(p: Plugin, v: string) {
  try { await api('PUT', `/api/plugins/${p.id}/placement`, { placement: v === p.suggested_placement ? '' : v }); say(t('Saved')); loadPanels(); } catch (e) { say((e as Error).message); }
}
async function save(p: Plugin) {
  try { await api('PUT', `/api/plugins/${p.id}/settings`, forms[p.id].value()); say(t('Saved')); } catch (e) { say((e as Error).message); }
}

// A plugin as a .zip (plugin.json at the top or in one folder). One that is there already is replaced after
// asking; its own files that the zip does not have (tokens, a cache) stay.
const uploading = ref(false);
const replacing = ref<{ data: string, name: string } | null>(null);
async function upload(e: Event) {
  const input = e.target as HTMLInputElement, f = input.files?.[0];
  input.value = '';
  if (!f) return;
  if (f.size > 35 * 1024 * 1024) { say(t('The zip is too big (at most 35 MB).')); return; }
  uploading.value = true;
  try {
    const data = await new Promise<string>((ok, no) => { const rd = new FileReader(); rd.onload = () => ok(String(rd.result).split(',')[1] || ''); rd.onerror = no; rd.readAsDataURL(f); });
    try { installed(await api('POST', '/api/plugins/upload', { data })); } catch (x) {
      const d = x instanceof ApiError ? x.data as { exists?: boolean, name?: string, version?: string, id?: string } | null : null;
      if (!(d && d.exists)) throw x;
      replacing.value = { data, name: d.name ? `${d.name}${d.version ? ' v' + d.version : ''}` : d.id || '' };
    }
  } catch (x) { say((x as Error).message); } finally { uploading.value = false; }
}
async function replace() {
  const r = replacing.value; replacing.value = null;
  if (!r) return;
  try { installed(await api('POST', '/api/plugins/upload', { data: r.data, replace: true })); } catch (x) { say((x as Error).message); }
}
function installed(x: { name: string, version?: string, replaced?: boolean, enabled?: boolean }) {
  say(t(x.replaced ? '“{name}” replaced with version {v}.' : x.enabled ? '“{name}” installed.' : '“{name}” installed. Switch it on below.', { name: x.name, v: x.version || '?' }));
  load();
}
const time = (s: number) => new Date(s * 1000).toLocaleTimeString(lang.value);
const PLACES: [string, string][] = [['sidebar', 'The left menu (a page of its own)'], ['settings', 'Settings → Account'], ['both', 'Both']];
</script>

<template>
  <div>
    <h1>{{ t('Plugins') }}</h1>
    <p class="dim">
      {{ t('Plugins add features: fill in movie details, send notifications, react to playback, add their own API actions. Put each plugin in its own folder inside ') }}<code>{{ folder }}</code>{{ t(' with a ') }}<code>plugin.json</code>{{ t(' file, then press Reload, or upload the folder as a .zip. Any language works (Python, shell, Node …).') }}
    </p>
    <div class="actions">
      <button @click="reload">{{ t('Reload plugins') }}</button>
      <label class="btn" :class="{ disabled: uploading }">{{ t('Upload plugin (.zip)') }}<input type="file" accept=".zip,application/zip" hidden :disabled="uploading" @change="upload"></label>
      <span v-if="uploading" class="dim">{{ t('Installing…') }}</span>
    </div>
    <p v-if="error" class="err">{{ error }}</p>
    <div v-if="note" class="toast">{{ note }}</div>
    <div v-if="errors.length" class="panel err"><div v-for="e in errors" :key="e">{{ e }}</div></div>

    <div v-if="plugins === null" class="skel line"></div>
    <p v-else-if="!plugins.length" class="empty">{{ t('No plugins installed yet.') }}</p>
    <section v-for="p in plugins" :key="p.id + ':' + rev" class="panel">
      <div class="plugin-head">
        <b>{{ p.name }}</b><span class="dim nowrap">v{{ p.version || '?' }}</span><span v-if="p.author" class="dim nowrap">{{ t('by {name}', { name: p.author }) }}</span>
        <span class="plugin-switch">
          <span class="nowrap" :class="p.enabled ? 'ok' : 'dim'">{{ p.enabled ? t('on') : t('off') }}</span>
          <button :class="{ primary: !p.enabled }" @click="toggle(p)">{{ p.enabled ? t('Switch off') : t('Switch on') }}</button>
        </span>
      </div>
      <p>{{ p.description }}</p>
      <div>{{ t('Listens to: ') }}<span v-for="x in p.hooks" :key="x" class="tag">{{ x }}</span></div>
      <div v-if="p.user_panel" class="form gap-top">
        <label :for="'pl-' + p.id">{{ t('Shown in') }}</label>
        <div>
          <select :id="'pl-' + p.id" :value="p.placement" @change="place(p, ($event.target as HTMLSelectElement).value)">
            <option v-for="[v, l] in PLACES" :key="v" :value="v">{{ t(l) }}{{ v === p.suggested_placement ? ' ' + t('(suggested by the plugin)') : '' }}</option>
          </select>
          <p class="dim hint">{{ t('Everyone can still unpin it from their own menu.') }}</p>
        </div>
      </div>
      <div class="gap-top">
        <PluginSettings :ref="(el: unknown) => { if (el) forms[p.id] = el as InstanceType<typeof PluginSettings> }" :settings="p.settings" :defaults="p.defaults" :choices="p.setting_choices" />
      </div>
      <div class="actions">
        <button @click="save(p)">{{ t('Save settings') }}</button>
        <button @click="forms[p.id]?.toggle()">{{ forms[p.id]?.asJson ? t('Edit as fields') : t('Edit as JSON') }}</button>
      </div>
    </section>

    <h2>{{ t('Recent plugin activity') }}</h2>
    <p v-if="!log.length" class="dim">{{ t('Nothing yet.') }}</p>
    <div v-else class="table-wrap">
      <table class="table">
        <tr v-for="(l, i) in log" :key="i">
          <td class="dim nowrap">{{ time(l.at) }}</td><td>{{ l.plugin }}</td><td>{{ l.event }}</td>
          <td :class="l.ok ? 'ok' : 'err'">{{ l.ok ? t('ok') : t('failed') }}</td><td class="dim break">{{ l.detail }}</td>
        </tr>
      </table>
    </div>
    <h2>{{ t('Events a plugin can listen to') }}</h2>
    <p><code v-for="e in events" :key="e" class="ev">{{ e }}</code></p>

    <Modal v-if="replacing" :title="t('Replace plugin')" @close="replacing = null">
      <p>{{ t('“{name}” is installed already. Replace it with the uploaded one? Its settings and its own files stay.', { name: replacing.name }) }}</p>
      <template #actions><button class="primary" @click="replace">{{ t('Replace') }}</button><button @click="replacing = null">{{ t('Cancel') }}</button></template>
    </Modal>
  </div>
</template>
