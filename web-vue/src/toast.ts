// A short message at the bottom of the window ("Saved", an error), shown by App.vue for a few seconds.
import { reactive } from 'vue';

export const toastState = reactive({ text: '', n: 0 });
let timer = 0;
export function toast(text: string, ms = 4000) {
  toastState.text = text; toastState.n++;
  clearTimeout(timer);
  timer = window.setTimeout(() => { toastState.text = ''; }, ms);
}
