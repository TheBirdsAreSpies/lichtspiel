<script lang="ts">
// The boxes open now, the top one last: Escape closes only that one.
const open: symbol[] = [];
</script>

<script setup lang="ts">
// A box over the page: a title, what goes in it, and buttons at the bottom (slot "actions").
// Escape and a click beside it close it.
import { onBeforeUnmount, onMounted, ref } from 'vue';

defineProps<{ title: string }>();
const emit = defineEmits<{ close: [] }>();
const box = ref<HTMLElement | null>(null);
const me = Symbol('modal');
const esc = (e: KeyboardEvent) => { if (e.key === 'Escape' && open[open.length - 1] === me) { e.stopPropagation(); emit('close'); } };
onMounted(() => {
  open.push(me);
  document.addEventListener('keydown', esc, true);
  // The first field gets the focus, else the box itself (so Tab stays in it).
  const first = box.value && box.value.querySelector<HTMLElement>('input, select, textarea');
  (first || box.value)?.focus();
});
onBeforeUnmount(() => { document.removeEventListener('keydown', esc, true); open.splice(open.indexOf(me), 1); });
</script>

<template>
  <Teleport to="body">
    <div class="modal-back" @mousedown.self="emit('close')">
      <div ref="box" class="modal" role="dialog" aria-modal="true" :aria-label="title" tabindex="-1">
        <h2>{{ title }}</h2>
        <slot />
        <div class="actions"><slot name="actions" /></div>
      </div>
    </div>
  </Teleport>
</template>
