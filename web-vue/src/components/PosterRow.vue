<script setup lang="ts">
// A row of posters that scrolls sideways, with arrow buttons for a mouse.
import { computed, ref } from 'vue';
import type { Item } from '../api';
import { t } from '../i18n';
import Icon from './Icon.vue';
import PosterCard from './PosterCard.vue';

// cont: the Continue Watching row (a title can be taken off it, from its menu).
const props = defineProps<{ title: string, items: Item[], wide?: boolean, cont?: boolean }>();
const gone = ref(new Set<number>());
const shown = computed(() => props.items.filter(i => !gone.value.has(i.id)));
const strip = ref<HTMLElement | null>(null);
const page = (dir: number) => { const el = strip.value; if (el) el.scrollBy({ left: dir * el.clientWidth * 0.8, behavior: 'smooth' }); };
</script>

<template>
  <section v-if="shown.length" class="row">
    <div class="row-head">
      <h2>{{ title }}</h2>
      <span class="row-arrows">
        <button class="iconbtn" :aria-label="t('Back')" @click="page(-1)"><Icon name="left" /></button>
        <button class="iconbtn" :aria-label="t('Next')" @click="page(1)"><Icon name="right" /></button>
      </span>
    </div>
    <div ref="strip" class="strip">
      <PosterCard v-for="it in shown" :key="it.id" :item="it" :wide="wide" :cont="cont" @removed="gone = new Set([...gone, $event])" />
    </div>
  </section>
</template>
