<script setup lang="ts">
// The API page (owner): everything the app does goes through the JSON API (GET /api/docs lists it), so
// scripts can do the same. One's own token (shown on request, rotated, extra tokens for scripts), the
// filter fields, and every address by group with a search.
import { computed, onMounted, ref } from 'vue';
import { api, get, renewTickets, saveToken, session } from '../api';
import { fmtDateTime } from '../format';
import { t } from '../i18n';
import { toast } from '../toast';
import AskDialog from '../components/AskDialog.vue';
import Modal from '../components/Modal.vue';

interface Route { method: string, path: string, description: string }
interface Docs { auth: string, groups: { name: string, routes: Route[] }[] }
interface Token { id: string, device?: string, created_at: number, current?: boolean }
interface Filters { fields: { field: string }[], url_shortcuts: Record<string, string> }

const docs = ref<Docs | null>(null);
const filters = ref<Filters | null>(null);
const tokens = ref<Token[] | null>(null);
const error = ref('');
const shown = ref(false);
const query = ref('');
const open = ref<Record<string, boolean>>({});

async function loadTokens() {
  try { tokens.value = (await get<{ tokens: Token[] }>('/api/tokens')).tokens; } catch (e) { error.value = (e as Error).message; }
}
onMounted(async () => {
  loadTokens();
  get<Filters>('/api/filters').then(f => { filters.value = f; }).catch(() => {});
  try { docs.value = await get<Docs>('/api/docs'); } catch (e) { error.value = (e as Error).message; }
});

// The groups with the addresses the search finds (all of them, opened, while searching).
const groups = computed(() => {
  const q = query.value.trim().toLowerCase();
  return (docs.value ? docs.value.groups : []).map(g => ({
    name: g.name, total: g.routes.length,
    routes: q ? g.routes.filter(r => `${r.method} ${r.path} ${r.description}`.toLowerCase().includes(q)) : g.routes,
  })).filter(g => g.routes.length);
});
const setAll = (v: boolean) => { open.value = Object.fromEntries((docs.value ? docs.value.groups : []).map(g => [g.name, v])); };
const isOpen = (name: string) => !!query.value.trim() || !!open.value[name];
function toggled(name: string, e: Event) { if (!query.value.trim()) open.value[name] = (e.target as HTMLDetailsElement).open; }

// ---- tokens
const revoking = ref<Token | null>(null);
async function revoke() {
  const k = revoking.value; if (!k) return;
  revoking.value = null;
  try { await api('DELETE', '/api/tokens/' + encodeURIComponent(k.id)); toast(t('Token revoked')); loadTokens(); } catch (e) { toast((e as Error).message, 6000); }
}
const rotating = ref<boolean | null>(null);
async function rotate() {
  const all = !!rotating.value;
  rotating.value = null;
  try {
    const r = await api<{ token: string, revoked: number }>('POST', '/api/tokens/rotate', { all });
    saveToken(r.token); shown.value = false; renewTickets();
    toast(all ? t('New token made; {n} other sign-in(s) ended', { n: r.revoked - 1 }) : t('New token made; the old one no longer works'));
    loadTokens();
  } catch (e) { toast((e as Error).message, 6000); }
}
const naming = ref(false);
const made = ref('');
async function newToken(name: string | null) {
  naming.value = false;
  if (name === null) return;
  try { made.value = (await api<{ token: string }>('POST', '/api/tokens', { device: name })).token; loadTokens(); } catch (e) { toast((e as Error).message, 6000); }
}
const tokenAsk = t('Name for the new token (e.g. "backup script")');
const example = computed(() => `curl -H "X-Token: <token>" ${location.origin}/library/sections`);
</script>

<template>
  <div>
    <h1>{{ t('API') }}</h1>
    <p v-if="docs">{{ t('Everything this app does goes through this JSON API, so other apps and scripts can do the same. ') }}{{ docs.auth }}.</p>
    <p v-if="error" class="err">{{ error }}</p>
    <section class="panel">
      <p>
        {{ t('Your token: ') }}<code>{{ shown ? session.token : '••••••••' }}</code>
        <span class="actions inline">
          <button class="small" @click="shown = !shown">{{ shown ? t('Hide') : t('Show') }}</button>
          <button class="small" @click="rotating = false">{{ t('Rotate token') }}</button>
          <button class="small" @click="rotating = true">{{ t('Rotate and sign out everywhere else') }}</button>
          <button class="small" @click="naming = true">{{ t('New token for a script') }}</button>
        </span>
      </p>
      <p class="dim">{{ t('Example: ') }}<code>{{ example }}</code></p>
      <div v-if="tokens === null" class="skel line"></div>
      <div v-else class="table-wrap">
        <table class="table">
          <tr v-for="k in tokens" :key="k.id">
            <td><code>{{ k.id }}</code></td>
            <td>{{ k.device || t('Web app') }}<span v-if="k.current" class="tag here">{{ t('this device') }}</span></td>
            <td class="dim nowrap">{{ fmtDateTime(k.created_at) }}</td>
            <td><button v-if="!k.current" class="small" @click="revoking = k">{{ t('Revoke') }}</button></td>
          </tr>
        </table>
      </div>
    </section>
    <section v-if="filters" class="panel">
      <b>{{ t('Filters') }}</b>
      <p class="dim">
        {{ t('Browsing, playlist contents and smart playlists share the same fields: ') }}<template v-for="(f, i) in filters.fields" :key="f.field">{{ i ? ' ' : '' }}<code>{{ f.field }}</code></template>{{ t('. In addresses: ') }}
        <template v-for="(v, k) in filters.url_shortcuts" :key="k"><code>{{ k }}</code> {{ v }}; </template>
      </p>
    </section>
    <div class="api-tools">
      <input v-model="query" type="search" :placeholder="t('Find an address…')" :aria-label="t('Find an address…')">
      <button class="small" @click="setAll(true)">{{ t('Open all') }}</button>
      <button class="small" @click="setAll(false)">{{ t('Close all') }}</button>
    </div>
    <div v-if="!docs && !error" class="skel line"></div>
    <details v-for="g in groups" :key="g.name" class="panel apigroup" :open="isOpen(g.name)" @toggle="toggled(g.name, $event)">
      <summary><b>{{ g.name }}</b><span class="dim"> · {{ g.total }}</span></summary>
      <div class="table-wrap">
        <table class="table">
          <tr v-for="r in g.routes" :key="r.method + r.path">
            <td class="api-m"><code :class="'m-' + r.method.toLowerCase()">{{ r.method }}</code></td>
            <td class="api-p"><code>{{ r.path }}</code></td>
            <td class="dim">{{ r.description }}</td>
          </tr>
        </table>
      </div>
    </details>
    <p v-if="docs && !groups.length" class="empty">{{ t('Nothing found.') }}</p>

    <Modal v-if="revoking" :title="t('Revoke this token? The device using it is signed out.')" @close="revoking = null">
      <p><code>{{ revoking.id }}</code> · {{ revoking.device || t('Web app') }}</p>
      <template #actions>
        <button class="danger" @click="revoke">{{ t('Revoke') }}</button>
        <button @click="revoking = null">{{ t('Cancel') }}</button>
      </template>
    </Modal>
    <Modal v-if="rotating !== null" :title="t('Rotate token')" @close="rotating = null">
      <p>{{ rotating ? t('Make a new token and sign out every other device and script?') : t('Make a new token? Scripts using the old one stop working.') }}</p>
      <template #actions>
        <button class="primary" @click="rotate">{{ t('Rotate token') }}</button>
        <button @click="rotating = null">{{ t('Cancel') }}</button>
      </template>
    </Modal>
    <AskDialog v-if="naming" :title="tokenAsk" :ok="t('Create')" @done="newToken" />
    <Modal v-if="made" :title="t('Copy the new token now; it is not shown again:')" @close="made = ''">
      <input :value="made" class="full" readonly @focus="($event.target as HTMLInputElement).select()">
      <template #actions><button class="primary" @click="made = ''">{{ t('Done') }}</button></template>
    </Modal>
  </div>
</template>
