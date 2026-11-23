<script setup lang="ts">
// The file of a title's page, as in the classic app: file type and size, picture, every audio track,
// bitrate, subtitles, path, parts, and how this browser plays it (GET …/decision).
import { computed, ref, watch } from 'vue';
import { get, type Item, type Media } from '../api';
import { fmtNum, fmtSize, langName } from '../format';
import { t } from '../i18n';

type Any = Record<string, any>; // eslint-disable-line @typescript-eslint/no-explicit-any
const props = defineProps<{ item: Item, media: Media, playId: number }>();
const m = computed(() => props.media as Any);

const CONTAINER_NAMES: Record<string, string> = { matroska: 'MKV', mov: 'MP4', mp4: 'MP4', mpegts: 'TS', avi: 'AVI', webm: 'WebM' };
const channels = (n?: number) => ({ 1: 'Mono', 2: t('Stereo'), 3: '2.1', 6: '5.1', 7: '6.1', 8: '7.1' } as Record<number, string>)[n || 0] || (n ? t('{n} channels', { n }) : '');
const fileType = computed(() => [CONTAINER_NAMES[String(m.value.container || '').toLowerCase()] || String(m.value.container || '').toUpperCase(), m.value.size ? fmtSize(m.value.size) : ''].filter(Boolean).join(' · '));
const video = computed(() => (m.value.videoCodec
  ? [String(m.value.videoCodec).toUpperCase(), `${m.value.width}×${m.value.height}`, m.value.bitDepth > 8 ? t('{n}-bit', { n: m.value.bitDepth }) : ''].filter(Boolean).join(' · ')
  : t('unknown (ffprobe not available)')));
// Every audio track on its own line: "German · AC3 · 5.1" (the file's main track when it lists none).
const audio = computed<string[]>(() => {
  const auds = ((m.value.streams || []) as Any[]).filter(s => s.kind === 'audio');
  if (!auds.length) return [m.value.audioCodec ? [String(m.value.audioCodec).toUpperCase(), channels(m.value.audioChannels)].filter(Boolean).join(' · ') : '–'];
  return auds.map(a => [a.language ? langName(a.language) : '', String(a.codec || '').toUpperCase(), channels(a.channels), a.title && a.title !== a.language ? a.title : ''].filter(Boolean).join(' · '));
});
const subs = computed(() => [...new Set(((m.value.subtitles || []) as Any[]).map(s => (s.language ? langName(s.language) : s.file || s.codec)))].join(', ') || t('none'));
const track = computed(() => props.item.type === 'track');

const decision = ref('…');
async function loadDecision() {
  decision.value = '…';
  try {
    const d = await get<{ method: string, reasons?: string[] }>(`/library/metadata/${props.playId}/decision`);
    const words: Record<string, string> = { direct: t('Plays the original file directly'), remux: t('Repackaged on the fly (picture untouched)'), transcode: t('Converted on the fly') };
    decision.value = (words[d.method] || d.method) + ((d.reasons || []).length ? ' — ' + (d.reasons || []).map(x => t(x)).join('; ') : '');
  } catch { decision.value = ''; }
}
watch(() => props.playId, loadDecision, { immediate: true });
</script>

<template>
  <div class="panel kv media-panel">
    <div>{{ t('File type') }}</div><div>{{ fileType }}</div>
    <template v-if="!track"><div>{{ t('Video') }}</div><div>{{ video }}</div></template>
    <div>{{ t('Audio') }}</div><div><div v-for="(a, i) in audio" :key="i">{{ a }}</div></div>
    <div>{{ t('Bitrate') }}</div><div>{{ m.bitrate ? fmtNum(m.bitrate / 1000, 1) + ' Mbit/s' : '–' }}</div>
    <template v-if="!track"><div>{{ t('Subtitles') }}</div><div>{{ subs }}</div></template>
    <template v-if="m.file"><div>{{ t('Path') }}</div><div class="break">{{ m.file }}</div></template>
    <template v-if="(m.Part || []).length > 1"><div>{{ t('Parts') }}</div><div>{{ t('{n} files, played one after the other', { n: m.Part.length }) }}</div></template>
    <template v-if="decision"><div>{{ t('In your browser') }}</div><div class="dim">{{ decision }}</div></template>
  </div>
</template>
