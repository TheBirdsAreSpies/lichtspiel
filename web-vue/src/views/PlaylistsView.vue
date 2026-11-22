<script setup lang="ts">
// Every playlist you can see (GET /playlists): your own, shared ones and the server's default ones, as
// pictures made of up to four posters. Filter by kind and owner, sort, find by name, start a new one.
import { computed, ref, watch } from 'vue';
import { useRoute, useRouter } from 'vue-router';
import { api, get, list, picture, type Item, type Playlist } from '../api';
import { fmtDur, kindName, plTitle } from '../format';
import { t } from '../i18n';
import { playAll } from '../queue';
import AskDialog from '../components/AskDialog.vue';
import SmartEditor from '../components/SmartEditor.vue';
import Icon from '../components/Icon.vue';

const route = useRoute();
const router = useRouter();
const KINDS: [string, string][] = [['', 'All kinds'], ['default', 'Default'], ['smart', 'Smart'], ['manual', 'Normal']];
const OWNERS: [string, string][] = [['', 'Everyone\'s'], ['me', 'Mine'], ['others', 'Shared with me'], ['server', 'From the server']];
const SORTS: [string, string][] = [['', 'Pinned first'], ['title', 'Name A–Z'], ['title:desc', 'Name Z–A'], ['updated_at:desc', 'Recently changed'],
  ['created_at:desc', 'Newest'], ['item_count:desc', 'Most items'], ['duration:desc', 'Longest']];

const str = (v: unknown) => (typeof v === 'string' ? v : '');
const kind = computed(() => str(route.query.kind));
const owner = computed(() => str(route.query.owner));
const sort = computed(() => str(route.query.sort));
const q = ref(str(route.query.title));
const pls = ref<Playlist[] | null>(null);
const error = ref('');
const asking = ref(false);
const smartNew = ref(false);

function go(change: Record<string, string>) {
  const next: Record<string, string> = { kind: kind.value, owner: owner.value, sort: sort.value, title: q.value.trim(), ...change };
  router.replace({ query: Object.fromEntries(Object.entries(next).filter(([, v]) => v)) });
}
let typing = 0;
watch(q, () => { clearTimeout(typing); typing = window.setTimeout(() => go({}), 300); });

let seq = 0;
watch(() => route.query, async () => {
  const p = new URLSearchParams({ sort: sort.value || 'pinned:desc,title' });
  if (kind.value) p.set('kind', kind.value);
  if (owner.value) p.set('owner', owner.value);
  const w = str(route.query.title);
  if (w) p.set('title', w);
  const mine = ++seq;
  error.value = '';
  try {
    const r = list<Playlist>(await get('/playlists?' + p));
    if (mine === seq) pls.value = r;
  } catch (e) { if (mine === seq) error.value = (e as Error).message; }
}, { immediate: true });

const info = (pl: Playlist) => [kindName(pl.kind), t('{n} items', { n: pl.leafCount }), fmtDur(pl.duration),
  pl.shared && pl.kind !== 'default' ? t('shared') : '', pl.pinned ? t('on home') : ''].filter(Boolean).join(' · ');

async function play(pl: Playlist) {
  try { playAll(router, list<Item>(await get(`/playlists/${pl.id}/items`))); } catch (e) { error.value = (e as Error).message; }
}
async function make(name: string | null) {
  asking.value = false;
  if (!name) return;
  try {
    const r = await api('POST', '/playlists', { title: name, kind: 'manual' });
    const pl = list<Playlist>(r)[0];
    if (pl) router.push('/playlist/' + pl.id);
  } catch (e) { error.value = (e as Error).message; }
}
</script>

<template>
  <div>
    <div class="lib-head">
      <h1>{{ t('Playlists') }}</h1>
      <div class="lib-bar">
        <input v-model="q" type="search" :placeholder="t('Find by name')" :aria-label="t('Find by name')">
        <select :value="kind" :aria-label="t('Kind')" @change="go({ kind: ($event.target as HTMLSelectElement).value })">
          <option v-for="[v, l] in KINDS" :key="v" :value="v">{{ t(l) }}</option>
        </select>
        <select :value="owner" :aria-label="t('Owner')" @change="go({ owner: ($event.target as HTMLSelectElement).value })">
          <option v-for="[v, l] in OWNERS" :key="v" :value="v">{{ t(l) }}</option>
        </select>
        <select :value="sort" :aria-label="t('Sort')" @change="go({ sort: ($event.target as HTMLSelectElement).value })">
          <option v-for="[v, l] in SORTS" :key="v" :value="v">{{ t(l) }}</option>
        </select>
        <button class="primary" @click="asking = true">{{ t('+ New playlist') }}</button>
        <button class="primary" @click="smartNew = true">{{ t('+ New smart playlist') }}</button>
      </div>
    </div>
    <p v-if="error" class="err">{{ error }}</p>
    <div v-if="pls === null" class="grid wide-grid"><div v-for="n in 8" :key="n" class="card ph"><div class="poster sq skel"></div></div></div>
    <div v-else-if="pls.length" class="grid wide-grid">
      <RouterLink v-for="pl in pls" :key="pl.id" :to="'/playlist/' + pl.id" class="card">
        <div class="poster sq">
          <div v-if="(pl.thumbs || []).length >= 4" class="mosaic"><img v-for="u in (pl.thumbs || []).slice(0, 4)" :key="u" :src="picture(u, 200)" class="in" loading="lazy" alt=""></div>
          <img v-else-if="(pl.thumbs || []).length" :src="picture((pl.thumbs || [])[0], 360)" class="in" loading="lazy" alt="">
          <div v-else class="initials">♪</div>
          <button v-if="pl.leafCount" class="play" :title="t('Play')" :aria-label="t('Play')" @click.prevent.stop="play(pl)"><Icon name="play" /></button>
        </div>
        <div class="t" :title="plTitle(pl)">{{ plTitle(pl) }}</div>
        <div class="s">{{ info(pl) }}</div>
      </RouterLink>
    </div>
    <p v-else class="empty">{{ t('No playlists match.') }}</p>
    <AskDialog v-if="asking" :title="t('Name of the new playlist')" :ok="t('Create')" @done="make" />
    <SmartEditor v-if="smartNew" @close="smartNew = false" />
  </div>
</template>
