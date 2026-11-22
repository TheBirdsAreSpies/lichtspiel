<script setup lang="ts">
// Reported problems (owner): what people reported about a title, to mark as done (or open again) and delete.
import { ref } from 'vue';
import { api, get } from '../api';
import { fmtDateTime } from '../format';
import { t } from '../i18n';
import { PROBLEMS } from '../reports';
import { toast } from '../toast';
import Modal from '../components/Modal.vue';

interface Report {
  id: number, kind: string, note?: string, user?: string, at: number, resolved?: boolean,
  item: { id: number, type?: string, title?: string, grandparentTitle?: string, parentIndex?: number, index?: number },
}
const reports = ref<Report[] | null>(null);
const error = ref('');
async function load() { try { reports.value = (await get<{ reports: Report[] }>('/api/reports')).reports; } catch (e) { error.value = (e as Error).message; } }
load();
const kindName = (k: string) => t((PROBLEMS.find(p => p[0] === k) || [k, k])[1]);
const what = (x: Report['item']) => (x.type === 'episode' ? `${x.grandparentTitle || ''} · ${t('S{n}', { n: x.parentIndex || 0 })}${t('E{n}', { n: x.index || 0 })} ${x.title}` : x.title || t('(removed)'));
async function toggle(p: Report) {
  try { await api('PUT', '/api/reports/' + p.id, { resolved: !p.resolved }); } catch (e) { toast((e as Error).message, 5000); }
  load();
}
const deleting = ref<Report | null>(null);
async function remove() {
  const p = deleting.value; deleting.value = null;
  if (!p) return;
  try { await api('DELETE', '/api/reports/' + p.id); } catch (e) { toast((e as Error).message, 5000); }
  load();
}
</script>

<template>
  <div>
    <h1>{{ t('Reported problems') }}</h1>
    <p v-if="error" class="err">{{ error }}</p>
    <div v-if="!reports && !error" class="skel line"></div>
    <p v-else-if="reports && !reports.length" class="empty">{{ t('No problems reported.') }}</p>
    <div v-else-if="reports" class="table-wrap panel">
      <table class="table">
        <tr><th>{{ t('What') }}</th><th>{{ t('Problem') }}</th><th>{{ t('Who') }}</th><th>{{ t('When') }}</th><th></th></tr>
        <tr v-for="p in reports" :key="p.id" :class="{ faded: p.resolved }">
          <td><RouterLink v-if="p.item.title" :to="'/item/' + p.item.id"><b>{{ what(p.item) }}</b></RouterLink><template v-else>{{ what(p.item) }}</template></td>
          <td>{{ kindName(p.kind) }}<div v-if="p.note" class="dim pre">{{ p.note }}</div></td>
          <td>{{ p.user || '–' }}</td>
          <td class="dim nowrap">{{ fmtDateTime(p.at) }}</td>
          <td class="nowrap">
            <button class="small" @click="toggle(p)">{{ p.resolved ? t('Reopen') : t('Done') }}</button>
            <button class="small danger" @click="deleting = p">{{ t('Delete') }}</button>
          </td>
        </tr>
      </table>
    </div>
    <Modal v-if="deleting" :title="t('Delete this report?')" @close="deleting = null">
      <template #actions><button class="danger" @click="remove">{{ t('Delete') }}</button><button @click="deleting = null">{{ t('Cancel') }}</button></template>
    </Modal>
  </div>
</template>
