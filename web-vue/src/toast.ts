// A short message at the bottom of the window ("Saved", an error), shown by App.vue for a few seconds;
// optionally with one button (such as "Undo").
import { reactive } from 'vue';

export interface ToastAction { label: string, run: () => void | Promise<void> }
export const toastState = reactive({ text: '', n: 0, action: null as ToastAction | null });
let timer = 0;
export function toast(text: string, ms = 4000, action?: ToastAction) {
  toastState.text = text; toastState.n++; toastState.action = action || null;
  clearTimeout(timer);
  timer = window.setTimeout(() => { toastState.text = ''; toastState.action = null; }, action ? Math.max(ms, 6000) : ms);
}
/** Runs the toast's button and closes the toast. */
export function toastRun() {
  const a = toastState.action; toastState.text = ''; toastState.action = null; clearTimeout(timer);
  if (a) Promise.resolve(a.run()).catch(e => toast((e as Error).message));
}
