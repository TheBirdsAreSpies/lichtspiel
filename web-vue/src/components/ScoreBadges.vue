<script setup lang="ts">
// Scores from review sites with their marks, as the source gives them; a badge links to the source's page.
import { computed } from 'vue';
import { lang, t } from '../i18n';
import { SCORE_MARKS } from '../scoreMarks';

interface Score { score?: string | number, count?: number, votes?: number, count_label?: string, url?: string, note?: string }
const props = defineProps<{ ratings?: Record<string, Score> | null }>();
const NAMES: [string, string][] = [['imdb', 'IMDb'], ['rotten_tomatoes', 'Rotten Tomatoes'], ['rotten_tomatoes_audience', 'Rotten Tomatoes audience'], ['metacritic', 'Metacritic'], ['tmdb', 'TMDB'], ['anilist', 'AniList']];
const badges = computed(() => {
  const r = props.ratings || {};
  return NAMES.filter(([k]) => r[k] && r[k].score).map(([k, n]) => {
    const s = r[k], raw = String(s.score), num = parseFloat(raw) || 0;
    const count = s.count ?? s.votes;
    const many = count ? t(s.count_label === 'reviews' ? '{n} reviews' : '{n} votes', { n: Number(count).toLocaleString(lang.value) }) : '';
    // (The audience score may come as 4.1/5 or 8/10: as a percentage for its mark.)
    const pct = k === 'rotten_tomatoes_audience' ? (raw.includes('/5') ? num * 20 : raw.includes('/10') ? num * 10 : num) : num;
    return { k, score: raw, sub: many || t(n), url: s.url, mark: SCORE_MARKS[k] ? SCORE_MARKS[k](pct) : '',
      title: `${t(n)}: ${raw}${many ? ' · ' + many : ''}${s.note ? ' — ' + s.note : ''}` };
  });
});
</script>

<template>
  <div v-if="badges.length" class="scores">
    <component :is="b.url ? 'a' : 'div'" v-for="b in badges" :key="b.k" class="score" :href="b.url" :target="b.url ? '_blank' : undefined" :rel="b.url ? 'noopener' : undefined" :title="b.title" :aria-label="b.title">
      <!-- (The marks are fixed SVG from scoreMarks.ts, not text from the server.) -->
      <span class="mark" v-html="b.mark"></span>
      <span class="num"><b>{{ b.score }}</b><span>{{ b.sub }}</span></span>
    </component>
  </div>
</template>
