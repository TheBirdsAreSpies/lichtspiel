<script setup lang="ts">
// A plugin's own page, reached from the left menu.
import { computed } from 'vue';
import { t } from '../i18n';
import { panels } from '../plugins';
import PluginPanel from '../components/PluginPanel.vue';

const props = defineProps<{ id: string }>();
const p = computed(() => panels.list.find(x => x.id === props.id));
</script>

<template>
  <div class="narrow-wide">
    <PluginPanel v-if="p" :id="p.id" :key="p.id" :name="p.name" />
    <p v-else-if="panels.loaded" class="empty">{{ t('This plugin is switched off or not shown to you.') }}</p>
  </div>
</template>
