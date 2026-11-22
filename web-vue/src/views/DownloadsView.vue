<script setup lang="ts">
// Downloads: each one with its quality, state and progress, a button to save it (or each part), to try a
// failed one again, and to remove it. Asked again every 2 s while something converts. The owner can see
// everyone's.
import { onBeforeUnmount, ref, watch } from 'vue';
import { api, get, picture, session } from '../api';
import { dlQualityName, loadDlCount, saveFile, type Download, type DownloadList } from '../downloads';
import { fmtDate, fmtSize } from '../format';
import { t } from '../i18n';
import { toast } from '../toast';
import Modal from '../components/Modal.vue';

const STATES: Record<string, string> = { queued: 'Waiting', converting: 'Converting', ready: 'Ready', failed: 'Failed' };
const list = ref<DownloadList | null>(null);
const error = ref('');
const all = ref(false);
let timer = 0, alive = true;
async function refresh() {
  clearTimeout(timer);
  try { list.value = await get<DownloadList>('/api/downloads' + (all.value ? '?all=1' : '')); error.value = ''; } catch (e) { error.value = (e as Error).message; }
  if (!alive) return;
  if (list.value) loadDlCount(all.value ? undefined : list.value);
  if (error.value || (list.value && list.value.converting + list.value.queued)) timer = window.setTimeout(refresh, 2000);
}
refresh();
watch(all, refresh);
onBeforeUnmount(() => { alive = false; clearTimeout(timer); });

const name = (d: Download) => {
  const x = d.item;
  if (!x || !x.title) return t('(no longer in the library)');
  return x.type === 'episode' ? `${x.grandparentTitle || ''} · ${t('S{n}', { n: x.parentIndex || 0 })}${t('E{n}', { n: x.index || 0 })} ${x.title}` : x.title;
};
const line = (d: Download) => [dlQualityName(d.quality), d.original && d.quality !== 'original' ? t('sent as it is (already fits)') : null,
  t(STATES[d.state] || d.state) + (d.state === 'converting' ? ` ${d.progress} %` : ''),
  d.size ? fmtSize(d.size) : null, all.value && d.user ? d.user : null,
  d.expires_at ? t('kept until {date}', { date: fmtDate(d.expires_at) }) : null].filter(Boolean).join(' · ');
const summary = () => {
  const l = list.value; if (!l) return '';
  const busy = l.converting + l.queued;
  return [busy ? t('{n} being converted', { n: busy }) : null, t('{n} ready', { n: l.ready }), l.failed ? t('{n} failed', { n: l.failed }) : null].filter(Boolean).join(' · ');
};

async function retry(d: Download) {
  try { await api('POST', `/api/downloads/${d.id}/retry`); } catch (e) { toast((e as Error).message, 6000); }
  refresh();
}
const removing = ref<Download | null>(null);
async function remove(d: Download, sure = false) {
  if (d.state === 'converting' && !sure) { removing.value = d; return; }
  removing.value = null;
  try { await api('DELETE', '/api/downloads/' + d.id); } catch (e) { toast((e as Error).message, 6000); }
  refresh();
}
async function clearFinished() {
  try { const r = await api<{ removed: number }>('DELETE', '/api/downloads?state=finished' + (all.value ? '&all=1' : '')); toast(t('{n} removed', { n: r.removed })); } catch (e) { toast((e as Error).message, 6000); }
  refresh();
}
</script>

<template>
  <div class="narrow-wide">
    <h1>{{ t('Downloads') }}</h1>
    <p v-if="list" class="dim">
      <template v-if="list.allowed">{{ t('Use the download button on the page of a movie, an episode, a season or a show.') }} {{ list.keep_days ? t('Converted copies are kept on the server for {n} days.', { n: list.keep_days }) : '' }}</template>
      <template v-else>{{ t('Downloads are switched off for your account. Ask the owner of the server.') }}</template>
    </p>
    <p v-if="error" class="err">{{ error }}</p>
    <div v-if="list" class="bar-tools">
      <span class="dim">{{ summary() }}</span>
      <span class="grow"></span>
      <label v-if="session.me?.admin" class="check"><input v-model="all" type="checkbox"> {{ t('Everyone’s downloads') }}</label>
      <button v-if="list.ready + list.failed" class="small" @click="clearFinished">{{ t('Clear finished') }}</button>
    </div>
    <div v-if="!list && !error" class="skel line"></div>
    <p v-else-if="list && !list.downloads.length" class="empty">{{ t('Nothing downloaded yet.') }}</p>
    <div v-else-if="list" class="dl-list">
      <div v-for="d in list.downloads" :key="d.id" class="dl-row">
        <img :src="picture(d.thumb, 80)" alt="" loading="lazy">
        <div class="grow">
          <RouterLink v-if="d.item && d.item.title" :to="'/item/' + d.item.id"><b>{{ name(d) }}</b></RouterLink><b v-else>{{ name(d) }}</b>
          <div class="dim">{{ line(d) }}</div>
          <div v-if="d.state === 'converting' || d.state === 'queued'" class="dlbar"><i :style="{ width: d.progress + '%' }"></i></div>
          <div v-if="d.state === 'failed' && d.error" class="err small-text">{{ t(d.error) }}</div>
        </div>
        <div class="actions">
          <template v-if="d.state === 'ready' && d.parts > 1">
            <button v-for="n in d.parts" :key="n" class="small primary" @click="saveFile(`${d.url}?part=${n - 1}`)">{{ t('Save part {n}', { n }) }}</button>
          </template>
          <button v-else-if="d.state === 'ready'" class="small primary" @click="saveFile(d.url)">{{ t('Save') }}</button>
          <button v-if="d.state === 'failed'" class="small" @click="retry(d)">{{ t('Try again') }}</button>
          <button class="small" :title="d.state === 'converting' ? t('Stop and remove') : t('Remove')" :aria-label="t('Remove')" @click="remove(d)">✕</button>
        </div>
      </div>
    </div>
    <Modal v-if="removing" :title="t('Stop converting “{title}” and remove it?', { title: name(removing) })" @close="removing = null">
      <template #actions>
        <button class="danger" @click="remove(removing, true)">{{ t('Remove') }}</button>
        <button @click="removing = null">{{ t('Cancel') }}</button>
      </template>
    </Modal>
  </div>
</template>
