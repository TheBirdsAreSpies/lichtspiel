<script setup lang="ts">
// Download a movie, an episode, a season or a show: pick a quality. The original is ready at once (one
// movie or episode is then saved right away); other qualities are converted on the server first and
// listed on the Downloads page with their progress.
import { computed, ref } from 'vue';
import { api, get, session, type Item } from '../api';
import { DL_QUALITIES, dlEstimate, dlQualityName, loadDlCount, saveFile, type DownloadList } from '../downloads';
import { fmtSize, isPlayable, seasonTitle } from '../format';
import { t } from '../i18n';
import { toast } from '../toast';
import Modal from './Modal.vue';

const props = defineProps<{ item: Item }>();
const emit = defineEmits<{ close: [] }>();
const it = props.item;
const playable = isPlayable(it);
const media = playable && it.Media ? it.Media[0] : null;
const leaves = playable ? 1 : it.leafCount || 0;
const ms = playable ? it.duration || 0 : 0;
const mayConvert = !!(session.me && (session.me.admin || session.me.allow_transcode !== false));
const quality = ref(mayConvert ? localStorage.getItem('vue-dlQuality') || 'original' : 'original');
const title = it.type === 'episode' && it.grandparentTitle ? `${it.grandparentTitle} · ${it.title}` : it.type === 'season' ? `${it.parentTitle || ''} · ${seasonTitle(it.title)}` : it.title;
// Never more than the original: a file that already fits is sent as it is.
function label(q: string) {
  const orig = media ? media.size || 0 : 0;
  const size = q === 'original' ? orig : orig ? Math.min(orig, dlEstimate(q, ms)) : dlEstimate(q, ms);
  return size ? `${dlQualityName(q)} · ${q === 'original' ? '' : '≈ '}${fmtSize(size)}` : dlQualityName(q);
}
const note = computed(() => (quality.value === 'original'
  ? t('The file as it is on the server. It is ready right away.')
  : t('Converted on the server to MP4 (H.264, stereo sound) that plays on every device. It is listed on the Downloads page with its progress; save it from there once it is ready. A file that is already small enough is sent as it is.')));

const busy = ref(false);
async function go() {
  const q = quality.value;
  localStorage.setItem('vue-dlQuality', q);
  busy.value = true;
  let r: { added: number, ready: number, queued: number, already: number, refused: number };
  try { r = await api('POST', '/api/downloads', { ids: [it.id], quality: q }); } catch (e) { busy.value = false; toast((e as Error).message, 6000); return; }
  emit('close');
  // One movie or episode in its original quality: saved right away.
  if (playable && r.ready === 1 && r.added === 1) {
    const list = await get<DownloadList>('/api/downloads');
    const d = list.downloads.find(x => x.item && x.item.id === it.id && x.quality === q);
    if (d && d.parts <= 1) { saveFile(d.url); toast(t('Saving the file — it is also on the Downloads page')); loadDlCount(list); return; }
  }
  const parts = [];
  if (r.ready) parts.push(t('{n} ready', { n: r.ready }));
  if (r.queued) parts.push(t('{n} being converted', { n: r.queued }));
  if (r.already) parts.push(t('{n} already there', { n: r.already }));
  if (r.refused) parts.push(t('{n} left out (you may not convert)', { n: r.refused }));
  toast(t('Downloads: {what}', { what: parts.join(', ') }), 5000);
  loadDlCount();
}
</script>

<template>
  <Modal :title="t('Download “{title}”', { title })" @close="emit('close')">
    <p v-if="!playable && leaves">{{ leaves === 1 ? t('1 video') : t('{n} videos', { n: leaves }) }}</p>
    <div class="form">
      <label for="dl-q">{{ t('Quality') }}</label>
      <select id="dl-q" v-model="quality">
        <option v-for="[q] in DL_QUALITIES" :key="q" :value="q" :disabled="q !== 'original' && !mayConvert">{{ label(q) }}</option>
      </select>
    </div>
    <p class="dim">{{ note }}</p>
    <p v-if="!mayConvert" class="dim">{{ t('You may not have videos converted, so only the original quality can be downloaded.') }}</p>
    <template #actions>
      <button class="primary" :disabled="busy" @click="go">{{ t('Download') }}</button>
      <RouterLink to="/downloads" class="btn" @click="emit('close')">{{ t('Open Downloads') }}</RouterLink>
      <button @click="emit('close')">{{ t('Cancel') }}</button>
    </template>
  </Modal>
</template>
