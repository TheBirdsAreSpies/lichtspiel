<script setup lang="ts">
// Server settings → Maintenance: backups (now, and putting one back), optimized versions, the server's log,
// and troubleshooting (optimize the library file, clean up, theme music, the report and the library file).
import { ref } from 'vue';
import { api, boot, get } from '../api';
import { saveFile } from '../downloads';
import { fmtDateTime } from '../format';
import { lang, t } from '../i18n';
import { toast } from '../toast';
import Modal from './Modal.vue';

const size = (n: number) => (n >= 1048576 ? (n / 1048576).toFixed(1) + ' MB' : Math.max(1, Math.round(n / 1024)) + ' KB');

// ---- backups
const last = ref<number | null>(null);
const backups = ref<{ name: string, time: number, size: number }[] | null>(null);
async function loadBackups() {
  try {
    const [st, r] = await Promise.all([get<{ settings: { maintenance_last?: { backup?: number } } }>('/api/settings'), get<{ backups: { name: string, time: number, size: number }[] }>('/api/maintenance/backups')]);
    last.value = (st.settings.maintenance_last && st.settings.maintenance_last.backup) || null; backups.value = r.backups;
  } catch (e) { toast((e as Error).message, 5000); }
}
loadBackups();
async function backup() {
  try { const x = await api<{ path: string }>('POST', '/api/maintenance/backup'); toast(t('Backup saved: {path}', { path: x.path }), 6000); loadBackups(); } catch (e) { toast((e as Error).message, 5000); }
}
const restoring = ref('');
async function restore() {
  const name = restoring.value; restoring.value = '';
  try { await api('POST', '/api/maintenance/restore', { name }); toast(t('Backup restored')); boot(); } catch (e) { toast((e as Error).message, 6000); }
}

// ---- optimized versions
const optimized = ref<{ id: number, title: string, quality?: string, size: number }[] | null>(null);
async function loadOptimized() { try { optimized.value = (await get<{ items: { id: number, title: string, quality?: string, size: number }[] }>('/api/optimized')).items; } catch (e) { toast((e as Error).message, 5000); } }
loadOptimized();
const dropping = ref(false);
async function dropOptimized() {
  dropping.value = false;
  try { const x = await api<{ removed: number }>('DELETE', '/api/optimized'); toast(t('{n} optimized version(s) deleted', { n: x.removed })); loadOptimized(); } catch (e) { toast((e as Error).message, 5000); }
}

// ---- the server's log: its own messages (problems with files, downloads, backups …)
const log = ref('');
const logBox = ref<HTMLElement | null>(null);
async function loadLog() {
  try {
    const r = await get<{ lines: { t: number, msg: string }[] }>('/api/log');
    log.value = r.lines.length ? r.lines.slice(-200).map(l => new Date(l.t * 1000).toLocaleString(lang.value) + '  ' + l.msg).join('\n') : t('Nothing has been logged yet.');
    setTimeout(() => { if (logBox.value) logBox.value.scrollTop = logBox.value.scrollHeight; });
  } catch (e) { log.value = (e as Error).message; }
}
loadLog();

// ---- troubleshooting
async function optimize() { try { const x = await api<{ before: number, after: number }>('POST', '/api/maintenance/optimize'); toast(t('Optimized: {a} → {b}', { a: size(x.before), b: size(x.after) }), 5000); } catch (e) { toast((e as Error).message, 5000); } }
async function cleanup() { try { const x = await api<{ freed: number, transcodes_kept?: boolean }>('POST', '/api/maintenance/cleanup'); toast(t('{n} freed', { n: size(x.freed) }) + (x.transcodes_kept ? ' — ' + t('conversions in use were kept') : ''), 5000); } catch (e) { toast((e as Error).message, 5000); } }
async function themes() {
  try { const x = await api<{ started: boolean, shows: number }>('POST', '/api/maintenance/themes'); toast(x.started ? t('Cutting the theme music of {n} shows — a message comes when it is done', { n: x.shows }) : t('Every show with a found intro has its theme music already'), 5000); } catch (e) { toast(t((e as Error).message), 6000); }
}
</script>

<template>
  <section class="panel">
    <h2>{{ t('Backups') }}</h2>
    <p class="dim">{{ last ? t('Last backup: {date}', { date: fmtDateTime(last) }) : t('No backup yet.') }}</p>
    <button class="primary" @click="backup">{{ t('Back up now') }}</button>
    <table v-if="backups && backups.length" class="table gap-top">
      <tr v-for="b in backups" :key="b.name">
        <td><code>{{ b.name }}</code></td><td class="dim nowrap">{{ fmtDateTime(b.time) }}</td><td class="dim nowrap">{{ size(b.size) }}</td>
        <td><button class="small danger" @click="restoring = b.name">{{ t('Restore') }}</button></td>
      </tr>
    </table>
    <p v-else-if="backups" class="dim">{{ t('There are no backups in the backup folder yet.') }}</p>
  </section>

  <section class="panel">
    <h2>{{ t('Optimized versions') }}</h2>
    <template v-if="optimized">
      <p class="dim">{{ optimized.length ? t('{n} optimized version(s), {size} in all.', { n: optimized.length, size: size(optimized.reduce((a, i) => a + i.size, 0)) }) : t('Nothing has been optimized yet. Open a title and choose More → Optimize.') }}</p>
      <p v-for="i in optimized.slice(0, 30)" :key="i.id"><RouterLink :to="'/item/' + i.id">{{ i.title }}</RouterLink> <span class="dim">{{ i.quality || '' }} · {{ size(i.size) }}</span></p>
      <p v-if="optimized.length > 30" class="dim">{{ t('… and {n} more', { n: optimized.length - 30 }) }}</p>
      <button v-if="optimized.length" class="danger" @click="dropping = true">{{ t('Delete all optimized versions') }}</button>
    </template>
  </section>

  <section class="panel">
    <h2>{{ t('Server log') }}</h2>
    <pre ref="logBox" class="log">{{ log }}</pre>
    <div class="actions">
      <button @click="loadLog">{{ t('Refresh') }}</button>
      <button @click="saveFile('/api/log/download')">{{ t('Export as text') }}</button>
      <button @click="saveFile('/api/log/download?gz=1')">{{ t('Export compressed (.gz)') }}</button>
    </div>
  </section>

  <section class="panel">
    <h2>{{ t('Troubleshooting') }}</h2>
    <div class="form">
      <label>{{ t('Optimize library file') }}</label>
      <div class="val"><button @click="optimize">{{ t('Optimize') }}</button><p class="dim hint">{{ t('Compacts the library file. Safe to do while the server runs.') }}</p></div>
      <label>{{ t('Clean up bundles') }}</label>
      <div class="val"><button @click="cleanup">{{ t('Clean up') }}</button><p class="dim hint">{{ t('Removes leftover conversions and cache files older than a week.') }}</p></div>
      <label>{{ t('Theme music from show intros') }}</label>
      <div class="val"><button @click="themes">{{ t('Cut now') }}</button><p class="dim hint">{{ t('Cuts the theme music of every show still waiting for it now, instead of 3 shows a minute in the maintenance window. One show again: “Cut theme music” in its menu.') }}</p></div>
      <label>{{ t('Report') }}</label>
      <div class="val"><button @click="saveFile('/api/maintenance/report')">{{ t('Download report') }}</button><p class="dim hint">{{ t('Version, system, libraries, settings (without keys) and plugin messages as a text file.') }}</p></div>
      <label>{{ t('Library file') }}</label>
      <div class="val"><button @click="saveFile('/api/maintenance/database')">{{ t('Download library file') }}</button><p class="dim hint">{{ t('A copy of the whole library: items, watch states, users and settings.') }}</p></div>
    </div>
  </section>

  <Modal v-if="restoring" :title="t('Restore')" @close="restoring = ''">
    <p>{{ t('Put this backup back? Libraries, users, settings and watch history return to how they were then. The current state is backed up first.') }}</p>
    <template #actions><button class="danger" @click="restore">{{ t('Restore') }}</button><button @click="restoring = ''">{{ t('Cancel') }}</button></template>
  </Modal>
  <Modal v-if="dropping" :title="t('Delete all optimized versions?')" @close="dropping = false">
    <template #actions><button class="danger" @click="dropOptimized">{{ t('Delete') }}</button><button @click="dropping = false">{{ t('Cancel') }}</button></template>
  </Modal>
</template>
