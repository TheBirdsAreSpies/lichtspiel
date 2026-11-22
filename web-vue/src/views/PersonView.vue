<script setup lang="ts">
// A person (an actor, a director …): photo, everything of yours they are in (GET /library/people/{key}),
// and from TMDB when the server has that switched on: biography, birth, more photos and the whole
// filmography, with what of it is in your libraries.
import { computed, ref, watch } from 'vue';
import { useRoute } from 'vue-router';
import { get, list, picture, type Item } from '../api';
import { lang, t } from '../i18n';
import Modal from '../components/Modal.vue';
import PosterCard from '../components/PosterCard.vue';

interface Credit { tmdb_id: number, type: string, title: string, year?: number, role?: string, poster?: string, item?: Item | null }
interface Details {
  name?: string, biography?: string, birthday?: string, deathday?: string, place_of_birth?: string, imdb_id?: string,
  photos: { url: string, full: string }[], credits: Credit[],
}
interface Person { key: string, name: string, thumb?: string, photo?: string, size: number }

const props = defineProps<{ id: string }>();
const route = useRoute();
const person = ref<Person | null>(null);
const items = ref<Item[]>([]);
const details = ref<Details | null>(null);
const detailsNote = ref('');
const error = ref('');
const photoBroken = ref(false);
const big = ref<number | null>(null);
const asList = ref((() => { try { return localStorage.getItem('vue-filmoView') === 'list'; } catch { return false; } })());
watch(asList, v => { try { localStorage.setItem('vue-filmoView', v ? 'list' : 'grid'); } catch { /* none */ } });

const nameParam = () => (typeof route.query.name === 'string' ? route.query.name : '');
async function loadDetails(refresh: boolean) {
  detailsNote.value = '';
  const p = person.value;
  try {
    details.value = await get<Details>(`/library/people/${encodeURIComponent(props.id)}/details?name=${encodeURIComponent((p && p.name) || nameParam())}${refresh ? '&refresh=1' : ''}`);
  } catch (e) { detailsNote.value = (e as Error).message; }
}
watch(() => props.id, async () => {
  person.value = null; details.value = null; error.value = ''; photoBroken.value = false;
  try {
    const r = await get<Person & { Metadata?: Item[] }>(`/library/people/${encodeURIComponent(props.id)}?name=${encodeURIComponent(nameParam())}`);
    person.value = r; items.value = list<Item>({ MediaContainer: r });
  } catch (e) { error.value = (e as Error).message; return; }
  loadDetails(false);
}, { immediate: true });

const photo = computed(() => { const p = person.value; return p ? (p.thumb ? picture(p.thumb, 400) : p.photo || '') : ''; });
const initials = computed(() => ((person.value && person.value.name) || '?').split(' ').map(w => w[0]).slice(0, 2).join(''));
const day = (s: string) => new Date(s + 'T00:00').toLocaleDateString(lang.value, { dateStyle: 'long' });
const facts = computed(() => {
  const d = details.value; if (!d) return [];
  const end = d.deathday ? new Date(d.deathday) : new Date();
  const age = d.birthday ? Math.floor((end.getTime() - new Date(d.birthday).getTime()) / 31557600000) : null;
  return [
    d.birthday ? t('Born {date}', { date: day(d.birthday) }) + (age !== null && !d.deathday ? ` (${age})` : '') : '',
    d.deathday ? t('Died {date}', { date: day(d.deathday) }) + (age !== null ? ` (${age})` : '') : '',
    d.place_of_birth || '',
  ].filter(Boolean);
});
// What you have first, then the rest.
const credits = computed(() => { const c = (details.value && details.value.credits) || []; return [...c.filter(x => x.item), ...c.filter(x => !x.item)]; });
const step = (d: number) => { if (big.value !== null) big.value += d; };
const mine = computed(() => credits.value.filter(c => c.item).length);
</script>

<template>
  <div>
    <p v-if="error" class="err">{{ error }}</p>
    <div v-else-if="!person" class="detail"><div class="poster skel"></div><div class="info"><div class="skel line"></div><div class="skel line short"></div></div></div>
    <template v-else>
      <div class="detail person-head">
        <div class="poster">
          <img v-if="photo && !photoBroken" :src="photo" alt="" referrerpolicy="no-referrer" @error="photoBroken = true">
          <div v-else class="initials">{{ initials }}</div>
        </div>
        <div class="info">
          <h1>{{ person.name || t('Unknown') }}</h1>
          <p class="dim">{{ person.size === 1 ? t('In 1 movie or show in your libraries') : t('In {n} movies and shows in your libraries', { n: person.size }) }}</p>
          <div v-if="facts.length" class="dim">
            <div v-for="f in facts" :key="f">{{ f }}</div>
          </div>
          <p v-if="details && details.biography" class="plot bio">{{ details.biography }}</p>
        </div>
      </div>

      <div v-if="items.length" class="grid">
        <div v-for="it in items" :key="it.id">
          <PosterCard :item="it" />
          <div v-if="it.role" class="dim role">{{ t('as {role}', { role: it.role }) }}</div>
        </div>
      </div>
      <p v-else class="empty">{{ t('Nothing in your libraries.') }}</p>

      <template v-if="details">
        <section v-if="details.photos.length > 1" class="row">
          <div class="row-head"><h2>{{ t('Photos') }}</h2></div>
          <div class="strip">
            <button v-for="(p, i) in details.photos" :key="p.url" class="photo" @click="big = i"><img :src="p.url" loading="lazy" alt="" referrerpolicy="no-referrer"></button>
          </div>
        </section>
        <section v-if="credits.length" class="row">
          <div class="row-head">
            <h2>{{ t('Filmography') }} <span class="dim count">{{ mine }}/{{ credits.length }} {{ t('in your libraries') }}</span></h2>
            <span class="seg">
              <button :class="{ on: !asList }" @click="asList = false">{{ t('Grid') }}</button>
              <button :class="{ on: asList }" @click="asList = true">{{ t('List') }}</button>
            </span>
          </div>
          <div v-if="asList" class="episodes">
            <div v-for="c in credits" :key="c.type + c.tmdb_id" class="ep li" :class="{ faded: !c.item }">
              <component :is="c.item ? 'RouterLink' : 'div'" :to="c.item ? '/item/' + c.item.id : undefined" class="li-thumb tall">
                <img v-if="c.item ? c.item.thumb : c.poster" :src="c.item ? picture(c.item.thumb, 120) : c.poster" loading="lazy" alt="" referrerpolicy="no-referrer">
              </component>
              <div class="grow">
                <RouterLink v-if="c.item" :to="'/item/' + c.item.id"><b>{{ c.title }}</b></RouterLink><b v-else>{{ c.title }}</b>
                <div class="dim">{{ [c.year, c.type === 'show' ? t('Show') : t('Movie'), c.role].filter(Boolean).join(' · ') }}</div>
              </div>
              <span class="dim">{{ c.item ? t('In your libraries') : t('Not in your libraries') }}</span>
            </div>
          </div>
          <div v-else class="grid">
            <template v-for="c in credits" :key="c.type + c.tmdb_id">
              <div v-if="c.item"><PosterCard :item="c.item" /><div v-if="c.role" class="dim role">{{ c.role }}</div></div>
              <div v-else class="card faded" :title="t('Not in your libraries')">
                <div class="poster"><img v-if="c.poster" :src="c.poster" class="in" loading="lazy" alt="" referrerpolicy="no-referrer"></div>
                <div class="t" :title="c.title">{{ c.title }}</div>
                <div class="s">{{ [c.year, c.role].filter(Boolean).join(' · ') }}</div>
              </div>
            </template>
          </div>
        </section>
        <div class="actions">
          <button @click="loadDetails(true)">{{ t('Reload details') }}</button>
          <a v-if="details.imdb_id" class="btn" :href="`https://www.imdb.com/name/${details.imdb_id}/`" target="_blank" rel="noreferrer">IMDb ↗</a>
        </div>
      </template>
      <p v-else-if="detailsNote" class="dim note">{{ detailsNote }}</p>

      <Modal v-if="big !== null && details" :title="person.name" @close="big = null">
        <img :src="details.photos[big].full" class="photo-big" alt="" referrerpolicy="no-referrer">
        <template #actions>
          <button :disabled="big === 0" @click="step(-1)">‹</button>
          <span class="dim">{{ big + 1 }} / {{ details.photos.length }}</span>
          <button :disabled="big === details.photos.length - 1" @click="step(1)">›</button>
          <button @click="big = null">{{ t('Close') }}</button>
        </template>
      </Modal>
    </template>
  </div>
</template>
