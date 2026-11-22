<script setup lang="ts">
// Settings: your own (account, playback, devices, Trakt, the watchlist feed), one tab each (/settings/<tab>).
// The server's settings are for admins and still in the classic web app.
import { computed } from 'vue';
import { session } from '../api';
import { t } from '../i18n';
import AccountPanel from '../components/settings/AccountPanel.vue';
import DevicesPanel from '../components/settings/DevicesPanel.vue';
import FeedPanel from '../components/settings/FeedPanel.vue';
import PlaybackPanel from '../components/settings/PlaybackPanel.vue';
import TraktPanel from '../components/settings/TraktPanel.vue';
import PluginPanel from '../components/PluginPanel.vue';
import { panels } from '../plugins';

const props = defineProps<{ tab?: string }>();
const TABS: [string, string][] = [['account', 'Account'], ['playback', 'Playback'], ['devices', 'Devices'], ['trakt', 'Trakt'], ['feed', 'Watchlist feed']];
const tab = computed(() => (TABS.some(([k]) => k === props.tab) ? props.tab : 'account'));
</script>

<template>
  <div class="settings">
    <h1>{{ t('Settings') }}</h1>
    <nav class="tabs flat">
      <RouterLink v-for="[k, l] in TABS" :key="k" :to="'/settings/' + k" custom v-slot="{ navigate }">
        <button :class="{ on: tab === k }" @click="navigate">{{ t(l) }}</button>
      </RouterLink>
      <a v-if="session.me?.admin" class="btn admin-link" href="/#/settings/general">{{ t('Server settings') }} ↗</a>
    </nav>
    <template v-if="tab === 'account'">
      <AccountPanel />
      <PluginPanel v-for="p in panels.list.filter(x => x.settings)" :id="p.id" :key="p.id" :name="p.name" />
    </template>
    <PlaybackPanel v-else-if="tab === 'playback'" />
    <DevicesPanel v-else-if="tab === 'devices'" />
    <TraktPanel v-else-if="tab === 'trakt'" />
    <FeedPanel v-else />
  </div>
</template>
