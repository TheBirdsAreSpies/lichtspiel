<script setup lang="ts">
// The people registered here who may see the room's video, to tick and ask in; they get the invitation in the app.
import { ref } from 'vue';
import { api, get } from '../api';
import { t } from '../i18n';
import { toast } from '../toast';

interface Person { id: number, name: string, in_room?: boolean, invited?: boolean, online?: boolean }
const props = defineProps<{ code: string }>();
const people = ref<Person[] | null>(null);
const error = ref('');
const picked = ref<number[]>([]);
get<{ people: Person[] }>(`/api/together/${props.code}/people`).then(r => { people.value = r.people.filter(p => !p.in_room); }).catch(e => { error.value = (e as Error).message; });

async function ask() {
  const list = (people.value || []).filter(p => picked.value.includes(p.id));
  if (!list.length) { toast(t('Tick who you want to watch with')); return; }
  try {
    await api('POST', `/api/together/${props.code}/invite`, { users: list.map(p => p.id) });
    for (const p of list) p.invited = true;
    picked.value = [];
    toast(t('Invited: {names}', { names: list.map(p => p.name).join(', ') }));
  } catch (e) { toast((e as Error).message, 5000); }
}
</script>

<template>
  <p v-if="error" class="err">{{ error }}</p>
  <p v-else-if="!people" class="dim">{{ t('Loading…') }}</p>
  <p v-else-if="!people.length" class="dim">{{ t('Nobody else here may see this title.') }}</p>
  <template v-else>
    <div class="checks">
      <label v-for="p in people" :key="p.id"><input v-model="picked" type="checkbox" :value="p.id" :disabled="p.invited"> {{ p.name }}
        <span class="dim">{{ p.invited ? ' · ' + t('invited') : p.online ? ' · ' + t('online') : '' }}</span></label>
    </div>
    <div class="gap-top"><button @click="ask">{{ t('Invite the ticked') }}</button></div>
  </template>
</template>
