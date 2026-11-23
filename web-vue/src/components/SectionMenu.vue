<script setup lang="ts">
// The "⋯" beside a library in the left menu (owner), as in the classic app: scan its files, its details again,
// analysis, who may see it, and the libraries page for its name, folders, trash and removal. The menu is put
// on the page itself (the left menu scrolls and would cut it off).
import { computed, nextTick, onBeforeUnmount, ref } from 'vue';
import { navList, setPinned } from '../nav';
import { useRouter } from 'vue-router';
import { api, get, list, session, type Section } from '../api';
import { poke } from '../activity';
import { t } from '../i18n';
import { toast } from '../toast';
import AccessDialog from './AccessDialog.vue';

// section: a library (its tools, for the owner); navKey: an entry of the left menu (pin or unpin it, for everyone).
const props = defineProps<{ section?: Section, navKey?: string }>();
const admin = computed(() => !!session.me?.admin && !!props.section);
const navEntry = computed(() => navList.value.find(e => e.key === (props.navKey || (props.section ? 's' + props.section.id : ''))));
const router = useRouter();
const open = ref(false);
const at = ref({ left: 0, top: 0 });
const btn = ref<HTMLButtonElement | null>(null);
const menu = ref<HTMLElement | null>(null);
const access = ref(false);

const outside = (e: Event) => { if (!menu.value?.contains(e.target as Node) && !btn.value?.contains(e.target as Node)) close(); };
function close() { open.value = false; document.removeEventListener('pointerdown', outside, true); removeEventListener('resize', close); }
async function toggle() {
  if (open.value) return close();
  const r = btn.value!.getBoundingClientRect();
  at.value = { left: r.right + 6, top: r.top };
  open.value = true;
  document.addEventListener('pointerdown', outside, true); addEventListener('resize', close);
  // Kept on the screen: beside the button where there is room, else below it, and not past the bottom.
  await nextTick();
  const m = menu.value; if (!m) return;
  const w = m.offsetWidth, h = m.offsetHeight;
  let { left, top } = at.value;
  if (left + w > innerWidth - 8) { left = Math.max(8, r.right - w); top = r.bottom + 4; }
  if (top + h > innerHeight - 8) top = Math.max(8, innerHeight - h - 8);
  at.value = { left, top };
}
onBeforeUnmount(close);

async function reloadSections() {
  try { session.sections = list<Section>(await get('/library/sections'), 'Directory'); } catch { /* the menu shows the old counts */ }
}
async function run(what: () => Promise<void>) {
  close();
  try { await what(); } catch (e) { toast((e as Error).message, 6000); }
}
const s = () => props.section!;
const scan = () => run(async () => {
  await api('POST', `/library/sections/${s().id}/refresh`);
  toast(t('Scanning {name}', { name: s().title })); poke();
  setTimeout(reloadSections, 800);
});
const refreshAll = () => run(async () => {
  const r = await api<{ queued?: number }>('POST', `/library/sections/${s().id}/refreshMetadata`);
  toast(t('Downloading details for {n} movies and shows again — progress is shown at the top', { n: r.queued || 0 }), 5000); poke();
});
const analyze = (deep: boolean) => run(async () => {
  const r = await api<{ videos?: number }>('POST', `/library/sections/${s().id}/${deep ? 'deepanalyze' : 'analyze'}`);
  toast(t('Analyzing {n} videos — progress is shown at the top', { n: r.videos || 0 }), 5000); poke();
});
</script>

<template>
  <button ref="btn" type="button" class="nav-more" :class="{ open }" :title="t('More')" :aria-label="t('More')" aria-haspopup="menu" :aria-expanded="open" @click.prevent.stop="toggle">⋯</button>
  <Teleport to="body">
    <div v-if="open" ref="menu" class="menu side-menu" role="menu" :style="{ left: at.left + 'px', top: at.top + 'px' }">
      <template v-if="admin">
      <button class="menu-row" role="menuitem" @click="scan">{{ t('Scan library files') }}</button>
      <button class="menu-row" role="menuitem" @click="refreshAll">{{ t('Refresh all metadata') }}</button>
      <button class="menu-row" role="menuitem" @click="analyze(false)">{{ t('Analyze') }}</button>
      <button class="menu-row" role="menuitem" @click="analyze(true)">{{ t('Deep analysis') }}</button>
      <hr>
      <button class="menu-row" role="menuitem" @click="close(); access = true">{{ t('Grant access…') }}</button>
      <button class="menu-row" role="menuitem" @click="close(); router.push('/libraries')">{{ t('Manage library') }}</button>
      </template>
      <template v-if="navEntry">
        <hr v-if="admin">
        <button class="menu-row" role="menuitem" @click="close(); setPinned(navEntry.key, navEntry.hidden)">{{ navEntry.hidden ? t('Pin') : t('Unpin') }}</button>
      </template>
    </div>
  </Teleport>
  <AccessDialog v-if="access && section" :section="section.id" @close="access = false" />
</template>
