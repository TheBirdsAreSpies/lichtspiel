<script setup lang="ts">
// Who may see a library (owner). Access is given per library; opened for a title, the box says that the
// title comes with everything else in its library.
import { computed, onMounted, ref } from 'vue';
import { useRouter } from 'vue-router';
import { api, get, session } from '../api';
import { t } from '../i18n';
import { toast } from '../toast';
import Modal from './Modal.vue';

const props = defineProps<{ section: number, forItem?: boolean }>();
const emit = defineEmits<{ close: [] }>();
const router = useRouter();
interface User { id: number, username: string, display_name?: string, admin?: boolean, allowed_sections: number[] | null }
const users = ref<(User & { on: boolean })[] | null>(null);
const name = computed(() => (session.sections.find(s => s.id === props.section) || { title: '#' + props.section }).title);
const may = (u: User) => u.allowed_sections === null || u.allowed_sections.includes(props.section);
onMounted(async () => {
  try {
    const r = await get<{ users: User[] } | User[]>('/api/users');
    users.value = (Array.isArray(r) ? r : r.users).filter(u => !u.admin).map(u => ({ ...u, on: may(u) }));
  } catch (e) { toast((e as Error).message, 6000); emit('close'); }
});
async function save() {
  try {
    for (const u of users.value || []) {
      if (u.on === may(u)) continue;
      const now = u.allowed_sections === null ? session.sections.map(s => s.id) : u.allowed_sections;
      await api('PATCH', '/api/users/' + u.id, { allowed_sections: u.on ? [...new Set([...now, props.section])] : now.filter(n => n !== props.section) });
    }
    toast(t('Saved')); emit('close');
  } catch (e) { toast((e as Error).message, 6000); }
}
</script>

<template>
  <Modal :title="t('Grant access')" @close="emit('close')">
    <p class="dim">{{ forItem ? t('Access is given per library: ticked people may see everything in “{name}”, including this title.', { name }) : t('Ticked people may see everything in “{name}”.', { name }) }}</p>
    <div v-if="users === null" class="skel line"></div>
    <div v-else-if="users.length" class="checks">
      <label v-for="u in users" :key="u.id"><input v-model="u.on" type="checkbox"> {{ u.display_name || u.username }}</label>
    </div>
    <p v-else>{{ t('No other users yet. Invite someone under Users & invites.') }}</p>
    <template #actions>
      <button v-if="users && users.length" class="primary" @click="save">{{ t('Save') }}</button>
      <button v-else-if="users" class="primary" @click="emit('close'); router.push('/users')">{{ t('Users & invites') }}</button>
      <button @click="emit('close')">{{ t('Cancel') }}</button>
    </template>
  </Modal>
</template>
