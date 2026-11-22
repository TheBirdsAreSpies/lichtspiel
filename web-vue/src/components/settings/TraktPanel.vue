<script setup lang="ts">
// Settings → Trakt: connect your Trakt account (a code to type in at trakt.tv/activate), sync now,
// disconnect. The server's Trakt app and the sync settings are for admins, in the classic web app.
import { onBeforeUnmount, onMounted, ref } from 'vue';
import { api, get, session } from '../../api';
import { lang, t } from '../../i18n';
import Modal from '../Modal.vue';

interface Report { error?: string, at?: number, pulled_watched?: number, pulled_ratings?: number, pulled_watchlist?: number,
  pushed_watched?: number, pushed_ratings?: number, pushed_watchlist?: number }
interface Trakt { configured: boolean, connected: boolean, name?: string, last_sent?: string, last_sync?: number, sync_enabled?: boolean,
  syncing?: boolean, report?: Report | null, app_url?: string, redirect_uri?: string }

const p = ref<Trakt | null>(null);
const error = ref('');
const note = ref('');
const code = ref<{ code: string, url: string, interval: number } | null>(null);
const disconnecting = ref(false);
let timer = 0, poll = 0;

async function load() {
  clearTimeout(timer);
  try { p.value = await get<Trakt>('/api/trakt'); } catch (e) { error.value = (e as Error).message; return; }
  if (p.value.syncing) timer = window.setTimeout(load, 3000);
}
onMounted(load);
onBeforeUnmount(() => { clearTimeout(timer); clearTimeout(poll); });

async function act(f: () => Promise<unknown>) { error.value = ''; try { await f(); } catch (e) { error.value = (e as Error).message; } }
const sync = () => act(async () => { await api('POST', '/api/trakt/sync'); note.value = t('Syncing with Trakt…'); timer = window.setTimeout(load, 2000); });
const disconnect = () => act(async () => { disconnecting.value = false; await api('POST', '/api/trakt/disconnect'); await load(); });

// The device login: asks again until the code was typed in, told no, or ran out.
const connect = () => act(async () => {
  code.value = await api('POST', '/api/trakt/connect');
  const ask = async () => {
    if (!code.value) return;
    try {
      const s = (await api<{ status: string }>('POST', '/api/trakt/poll')).status;
      if (s === 'waiting') { poll = window.setTimeout(ask, Math.max(2, code.value.interval) * 1000); return; }
      code.value = null;
      note.value = s === 'connected' ? t('Connected to Trakt.') : s === 'denied' ? t('Trakt was told no.') : t('The code has expired. Try again.');
      load();
    } catch (e) { code.value = null; error.value = (e as Error).message; }
  };
  poll = window.setTimeout(ask, Math.max(2, code.value!.interval) * 1000);
});
function cancel() { code.value = null; clearTimeout(poll); }

const when = (s?: number) => (s ? new Date(s * 1000).toLocaleString(lang.value, { dateStyle: 'medium', timeStyle: 'short' }) : t('never'));
function summary(r?: Report | null) {
  if (!r || !r.at) return '';
  return t('from Trakt: {w} watched, {r} ratings, {l} on the watchlist; to Trakt: {w2} watched, {r2} ratings, {l2} on the watchlist',
    { w: r.pulled_watched || 0, r: r.pulled_ratings || 0, l: r.pulled_watchlist || 0, w2: r.pushed_watched || 0, r2: r.pushed_ratings || 0, l2: r.pushed_watchlist || 0 });
}
</script>

<template>
  <section class="panel">
    <h2>Trakt <span v-if="p && p.connected" class="ok-msg small">{{ t('connected') }}</span></h2>
    <p v-if="error" class="err">{{ error }}</p>
    <div v-if="!p && !error" class="skel line"></div>
    <template v-else-if="p">
      <p v-if="!p.configured">{{ t('Trakt is not set up yet') }}. {{ session.me?.admin ? t('Set up the Trakt app in the classic web app (Settings → Account).') : t('An admin needs to create a Trakt app first.') }}</p>
      <template v-else-if="!p.connected">
        <p>{{ t('Not connected. Connect your Trakt account and what you watch here shows up on Trakt.') }}</p>
        <div class="actions"><button class="primary" @click="connect">{{ t('Connect Trakt') }}</button></div>
      </template>
      <template v-else>
        <p>{{ t('Connected as {name}.', { name: p.name || '?' }) }}</p>
        <p v-if="p.last_sent" class="dim">{{ t('Last sent: {what}', { what: p.last_sent }) }}</p>
        <p v-if="p.sync_enabled || p.last_sync" class="dim">
          {{ t('Last sync: {when}', { when: when(p.last_sync) }) }}
          <span v-if="p.report && p.report.error" class="err"> · {{ p.report.error }}</span>
          <template v-else-if="summary(p.report)"> · {{ summary(p.report) }}</template>
        </p>
        <div class="actions">
          <button class="primary" :disabled="p.syncing" @click="sync">{{ p.syncing ? t('Syncing…') : t('Sync now') }}</button>
          <button class="danger" @click="disconnecting = true">{{ t('Disconnect') }}</button>
        </div>
      </template>
      <p v-if="note" class="dim">{{ note }}</p>
      <p v-if="session.me?.admin && p.configured" class="dim small"><a class="link" href="/#/settings/account">{{ t('The Trakt app and the sync settings are in the classic web app.') }} ↗</a></p>
    </template>

    <Modal v-if="code" :title="t('Connect Trakt')" @close="cancel">
      <p>{{ t('Open {url} and enter this code:', { url: code.url }) }}</p>
      <div class="big-code">{{ code.code }}</div>
      <p><a :href="code.url" target="_blank" rel="noopener" class="link">{{ code.url }}</a></p>
      <p class="dim">{{ t('Waiting until the code is entered…') }}</p>
      <template #actions><button @click="cancel">{{ t('Cancel') }}</button></template>
    </Modal>
    <Modal v-if="disconnecting" :title="t('Disconnect your Trakt account?')" @close="disconnecting = false">
      <template #actions>
        <button class="danger" @click="disconnect">{{ t('Disconnect') }}</button>
        <button @click="disconnecting = false">{{ t('Cancel') }}</button>
      </template>
    </Modal>
  </section>
</template>
