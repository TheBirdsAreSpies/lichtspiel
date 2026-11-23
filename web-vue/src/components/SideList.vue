<script setup lang="ts">
// The libraries part of the left menu (nav.ts): pinned entries, the rest under "More" (open while one of them
// is shown), each with its "⋯"; the heading's "⋯" edits the menu (drag, arrows, pin and unpin), pins all,
// or goes back to the standard order.
import { computed, onBeforeUnmount, ref } from 'vue';
import { useRoute, useRouter } from 'vue-router';
import { session, type Section } from '../api';
import { t } from '../i18n';
import { navList, putNav, saveNav, setPinned, type NavEntry } from '../nav';
import Icon from './Icon.vue';
import SectionMenu from './SectionMenu.vue';

const route = useRoute();
const router = useRouter();
const editing = ref(false);
const pinned = computed(() => navList.value.filter(e => !e.hidden));
const off = computed(() => navList.value.filter(e => e.hidden));
const moreKept = ref((() => { try { return localStorage.getItem('vue-navMore') === '1'; } catch { return false; } })());
const isOn = (e: NavEntry) => route.path === e.to || route.path.startsWith(e.to + '/') || (e.key === 'playlists' && route.name === 'playlist');
const moreOpen = computed(() => moreKept.value || off.value.some(isOn));
function toggleMore() {
  moreKept.value = !moreOpen.value;
  try { localStorage.setItem('vue-navMore', moreKept.value ? '1' : '0'); } catch { /* none */ }
}
// The number beside a library: its shows, or its videos.
const count = (s?: Section) => (!s ? '' : s.type === 'show' ? (s.count ? t('{n} shows', { n: s.count }) : '') : s.videoCount ?? s.count ?? '');

// ---- the heading's menu
const headOpen = ref(false);
const outside = (e: Event) => { if (!(e.target as Element).closest('.navhead')) headOpen.value = false; };
function toggleHead() {
  headOpen.value = !headOpen.value;
  if (headOpen.value) setTimeout(() => document.addEventListener('click', outside), 0); else document.removeEventListener('click', outside);
}
onBeforeUnmount(() => document.removeEventListener('click', outside));
const sb = () => session.me?.sidebar || {};
function headRun(what: string) {
  headOpen.value = false; document.removeEventListener('click', outside);
  if (what === 'edit') editing.value = true;
  else if (what === 'pinAll') putNav({ order: sb().order || [], hidden: [] });
  else if (what === 'standard') putNav({ order: [], hidden: [] });
  else if (what === 'add') router.push('/libraries');
}

// ---- editing: drag a row, or the arrows; ✕ unpins, + pins again
function move(e: NavEntry, d: number) {
  const list = navList.value.map(x => ({ ...x }));
  const group = list.filter(x => x.hidden === e.hidden), i = group.findIndex(x => x.key === e.key);
  const a = list.findIndex(x => x.key === e.key), b = list.findIndex(x => x.key === group[i + d].key);
  [list[a], list[b]] = [list[b], list[a]];
  saveNav(list);
}
const dragKey = ref(''), over = ref<{ key: string, below: boolean } | null>(null);
const below = (ev: DragEvent) => { const r = (ev.currentTarget as HTMLElement).getBoundingClientRect(); return ev.clientY > r.top + r.height / 2; };
function drop(e: NavEntry, ev: DragEvent) {
  const under = below(ev); over.value = null;
  const list = navList.value.map(x => ({ ...x }));
  const from = list.find(x => x.key === dragKey.value);
  if (!from || from.key === e.key) return;
  list.splice(list.indexOf(from), 1);
  // Dropped among the pinned entries it is pinned, among the others unpinned.
  from.hidden = e.hidden;
  list.splice(list.findIndex(x => x.key === e.key) + (under ? 1 : 0), 0, from);
  saveNav(list);
}
const groupOf = (e: NavEntry) => (e.hidden ? off.value : pinned.value);
</script>

<template>
  <div class="nav-head navhead">
    <span>{{ t('Libraries') }}</span>
    <button v-if="editing" class="small primary" @click="editing = false">{{ t('Done') }}</button>
    <span v-else class="head-wrap">
      <button class="navhead-more" :title="t('Edit the menu')" :aria-label="t('Edit the menu')" :aria-expanded="headOpen" @click="toggleHead">⋯</button>
      <div v-if="headOpen" class="menu navhead-menu" role="menu">
        <button class="menu-row" role="menuitem" @click="headRun('edit')">{{ t('Edit the menu') }}</button>
        <button v-if="off.length" class="menu-row" role="menuitem" @click="headRun('pinAll')">{{ t('Pin all') }}</button>
        <button v-if="(sb().order || []).length || (sb().hidden || []).length" class="menu-row" role="menuitem" @click="headRun('standard')">{{ t('Back to the standard order') }}</button>
        <template v-if="session.me?.admin"><hr><button class="menu-row" role="menuitem" @click="headRun('add')">{{ t('Add a library…') }}</button></template>
      </div>
    </span>
  </div>
  <template v-if="editing">
    <p class="dim navhint">{{ t('Drag to sort, or use the arrows. ✕ unpins, + pins again.') }}</p>
    <template v-for="(e, n) in [...pinned, ...off]" :key="e.key">
      <div v-if="e.hidden && n === pinned.length" class="edit-sep">{{ t('NOT PINNED') }}</div>
      <div class="edit-row" :class="{ off: e.hidden, drag: dragKey === e.key, 'over-top': over && over.key === e.key && !over.below, 'over-bottom': over && over.key === e.key && over.below }" draggable="true"
        @dragstart="dragKey = e.key; $event.dataTransfer && ($event.dataTransfer.effectAllowed = 'move')" @dragend="dragKey = ''; over = null"
        @dragover.prevent="over = { key: e.key, below: below($event) }" @dragleave="over = null" @drop.prevent="drop(e, $event)">
        <span class="grip" aria-hidden="true">⠿</span><span class="n" :title="e.title">{{ e.title }}</span>
        <button :title="t('Move up')" :aria-label="t('Move up')" :disabled="groupOf(e)[0].key === e.key" @click="move(e, -1)">↑</button>
        <button :title="t('Move down')" :aria-label="t('Move down')" :disabled="groupOf(e)[groupOf(e).length - 1].key === e.key" @click="move(e, 1)">↓</button>
        <button :title="e.hidden ? t('Pin') : t('Unpin')" :aria-label="e.hidden ? t('Pin') : t('Unpin')" @click="setPinned(e.key, e.hidden)">{{ e.hidden ? '+' : '✕' }}</button>
      </div>
    </template>
  </template>
  <template v-else>
    <div v-for="e in pinned" :key="e.key" class="nav-item" :class="{ admin: true }">
      <RouterLink :to="e.to" class="nav" :class="{ on: isOn(e) }" :title="e.title"><Icon :name="e.icon" /><span>{{ e.title }}</span><small v-if="count(e.sec)" class="nav-num">{{ count(e.sec) }}</small></RouterLink>
      <SectionMenu :section="e.sec" :nav-key="e.key" />
    </div>
    <template v-if="off.length">
      <a href="#" class="nav morehead" :title="t('More')" @click.prevent="toggleMore"><Icon name="more" /><span>{{ t('More') }}</span><small class="nav-num">{{ (moreOpen ? '▾ ' : '▸ ') + off.length }}</small></a>
      <template v-if="moreOpen">
        <div v-for="e in off" :key="e.key" class="nav-item" :class="{ admin: true }">
          <RouterLink :to="e.to" class="nav" :class="{ on: isOn(e) }" :title="e.title"><Icon :name="e.icon" /><span>{{ e.title }}</span><small v-if="count(e.sec)" class="nav-num">{{ count(e.sec) }}</small></RouterLink>
          <SectionMenu :section="e.sec" :nav-key="e.key" />
        </div>
      </template>
    </template>
  </template>
</template>
