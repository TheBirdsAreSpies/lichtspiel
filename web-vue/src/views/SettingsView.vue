<script setup lang="ts">
// Settings: your own (account, playback, devices, Trakt, the watchlist feed; the owner also webhooks), one tab
// each (/settings/<tab>).
// The server's settings are for the owner, on their own page (/server).
import { computed, ref } from 'vue';
import { session } from '../api';
import { t } from '../i18n';
import AccountPanel from '../components/settings/AccountPanel.vue';
import DevicesPanel from '../components/settings/DevicesPanel.vue';
import FeedPanel from '../components/settings/FeedPanel.vue';
import PlaybackPanel from '../components/settings/PlaybackPanel.vue';
import TraktPanel from '../components/settings/TraktPanel.vue';
import WebhooksPanel from '../components/settings/WebhooksPanel.vue';
import PluginPanel from '../components/PluginPanel.vue';
import { panels } from '../plugins';
import { serverMatches, TABS as SERVER_TABS } from '../serverSettings';

const props = defineProps<{ tab?: string }>();
const ALL: [string, string][] = [['account', 'Account'], ['playback', 'Playback'], ['devices', 'Devices'], ['trakt', 'Trakt'], ['feed', 'Watchlist feed'], ['webhooks', 'Webhooks']];
const TABS = computed(() => ALL.filter(([k]) => k !== 'webhooks' || (session.me && session.me.admin)));
// The search: what each tab holds (its settings' names), and for the owner the server's settings too.
const HOLDS: Record<string, string[]> = {
  account: ['Your account', 'User name', 'Full name', 'E-mail address', 'Password', 'Change my password', 'Two-step sign-in', 'Sign in automatically', 'Language of this app',
    'This app', 'Appearance', 'Accent color', 'Time format', 'Level the volume', 'Play theme music', 'Theme music repeats', 'Remember the last page'],
  playback: ['Video quality', 'Play smaller videos in original quality', 'Direct play', 'Watched indicator', 'Show the watched mark on', 'Audio and subtitles', 'Preferred audio language',
    'Preferred subtitle language', 'Subtitle mode', 'Subtitles for the hard of hearing (SDH)', 'Forced subtitles', 'Burn in subtitles', 'Subtitle size', 'Subtitle background', 'Subtitle position', 'Subtitle color'],
  devices: ['Active devices', 'Sign in a device', 'Sign out'],
  trakt: ['Trakt', 'Connect Trakt', 'Sync now'],
  feed: ['Watchlist feed'],
  webhooks: ['Webhooks', 'Addresses', 'Events', 'Discord style', 'Send a test message'],
};
const query = ref('');
const found = computed(() => {
  const q = query.value.trim().toLowerCase();
  if (!q) return null;
  const mine = TABS.value.flatMap(([k, l]) => (HOLDS[k] || []).filter(x => t(x).toLowerCase().includes(q) || x.toLowerCase().includes(q)).map(x => ({ tab: k, tabName: t(l), name: t(x) })));
  const server = session.me && session.me.admin ? serverMatches(q, t) : [];
  return { mine, server };
});
const serverName = (k: string) => t((SERVER_TABS.find(x => x[0] === k) || [k, k])[1]);
const tab = computed(() => (TABS.value.some(([k]) => k === props.tab) ? props.tab : 'account'));
</script>

<template>
  <div class="settings">
    <h1>{{ t('Settings') }}</h1>
    <nav class="tabs flat">
      <RouterLink v-for="[k, l] in TABS" :key="k" :to="'/settings/' + k" custom v-slot="{ navigate }">
        <button :class="{ on: tab === k }" @click="navigate">{{ t(l) }}</button>
      </RouterLink>
      <RouterLink v-if="session.me?.admin" class="btn admin-link" to="/server">{{ t('Server settings') }}</RouterLink>
      <input v-model="query" type="search" class="set-search" :placeholder="t('Search the settings…')" :aria-label="t('Search the settings…')">
    </nav>
    <div v-if="found" class="set-found">
      <p v-if="!found.mine.length && !found.server.length" class="empty">{{ t('No setting matches “{q}”.', { q: query.trim() }) }}</p>
      <div v-if="found.mine.length" class="list">
        <RouterLink v-for="(f, i) in found.mine" :key="i" class="li" :to="'/settings/' + f.tab" @click="query = ''"><b class="grow">{{ f.name }}</b><span class="dim">{{ f.tabName }}</span></RouterLink>
      </div>
      <template v-if="found.server.length">
        <h2>{{ t('Server settings') }}</h2>
        <div class="list">
          <RouterLink v-for="[k, , n] in found.server" :key="k" class="li" :to="{ path: '/server/' + k, query: { q: query.trim() } }"><b class="grow">{{ serverName(k) }}</b><span class="dim">{{ n }}</span></RouterLink>
        </div>
      </template>
    </div>
    <template v-else>
    <template v-if="tab === 'account'">
      <AccountPanel />
      <PluginPanel v-for="p in panels.list.filter(x => x.settings)" :id="p.id" :key="p.id" :name="p.name" />
    </template>
    <PlaybackPanel v-else-if="tab === 'playback'" />
    <DevicesPanel v-else-if="tab === 'devices'" />
    <TraktPanel v-else-if="tab === 'trakt'" />
    <WebhooksPanel v-else-if="tab === 'webhooks'" />
    <FeedPanel v-else />
    </template>
  </div>
</template>
