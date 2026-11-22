<script setup lang="ts">
// Settings → Webhooks (owner): the settings of the "Webhook notify" plugin in plain words: on or off, the
// addresses, which events, Discord style, and a test message.
import { ref } from 'vue';
import { api, get } from '../../api';
import { t } from '../../i18n';
import { toast } from '../../toast';

interface Plugin { id: string, enabled: boolean, settings?: { urls?: string[], events?: string[], discord_style?: boolean } }
const EVENTS: [string, string][] = [['library.item.added', 'New movies and episodes'], ['playback.started', 'Playback started'], ['playback.watched', 'Playback finished'],
  ['user.invited', 'Someone was invited'], ['user.joined', 'Someone joined'], ['library.scan.finished', 'A scan finished']];
const state = ref<'loading' | 'missing' | 'ok'>('loading');
const enabled = ref(false);
const urls = ref('');
const events = ref<string[]>([]);
const discord = ref(false);
const error = ref('');
get<{ plugins: Plugin[] }>('/api/plugins').then(r => {
  const pl = r.plugins.find(p => p.id === 'webhook-notify');
  if (!pl) { state.value = 'missing'; return; }
  const c = pl.settings || {};
  enabled.value = pl.enabled; urls.value = (c.urls || []).join('\n'); events.value = c.events || []; discord.value = !!c.discord_style;
  state.value = 'ok';
}).catch(e => { error.value = (e as Error).message; });

async function onOff() {
  try { await api('POST', `/api/plugins/webhook-notify/${enabled.value ? 'enable' : 'disable'}`); toast(t('Saved')); } catch (e) { toast((e as Error).message, 5000); }
}
async function save(quiet = false) {
  await api('PUT', '/api/plugins/webhook-notify/settings', { urls: urls.value.split(/\s+/).filter(Boolean), events: events.value, discord_style: discord.value });
  if (!quiet) toast(t('Saved'));
}
async function trySave() { try { await save(); } catch (e) { toast((e as Error).message, 5000); } }
async function test() {
  try { await save(true); const x = await api<{ error?: string }>('POST', '/api/plugins/webhook-notify/call/test'); toast(x.error || t('Test message sent'), 4000); } catch (e) { toast((e as Error).message, 5000); }
}
</script>

<template>
  <p v-if="error" class="err">{{ error }}</p>
  <section v-else-if="state === 'missing'" class="panel"><p>{{ t('The “Webhook notify” plugin is not installed. See Plugins.') }}</p></section>
  <section v-else-if="state === 'ok'" class="panel">
    <h2>{{ t('Webhooks') }}</h2>
    <div class="form">
      <span></span>
      <div class="val">
        <label class="check"><input v-model="enabled" type="checkbox" @change="onOff"> {{ t('Send messages') }}</label>
        <p class="dim hint">{{ t('Switches the “Webhook notify” plugin on or off.') }}</p>
      </div>
      <label for="wh-urls">{{ t('Addresses') }}</label>
      <div class="val"><textarea id="wh-urls" v-model="urls" rows="4" placeholder="https://example.com/hook"></textarea><p class="dim hint">{{ t('One per line. Each gets a JSON message.') }}</p></div>
      <label>{{ t('Events') }}</label>
      <div class="val"><label v-for="[k, l] in EVENTS" :key="k" class="check"><input v-model="events" type="checkbox" :value="k"> {{ t(l) }}</label></div>
      <span></span>
      <div class="val">
        <label class="check"><input v-model="discord" type="checkbox"> {{ t('Discord style') }}</label>
        <p class="dim hint">{{ t('Sends a message with “content”, which Discord and similar services understand.') }}</p>
      </div>
    </div>
    <div class="actions">
      <button class="primary" @click="trySave">{{ t('Save') }}</button>
      <button @click="test">{{ t('Send a test message') }}</button>
    </div>
  </section>
  <div v-else class="skel line"></div>
</template>
