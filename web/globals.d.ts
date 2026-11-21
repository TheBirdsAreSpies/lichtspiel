// Types for the type check only (npm run typecheck); the browser never loads this file.

// The pages are plain HTML: what the app looks up with querySelector is an HTML element.
interface ParentNode {
  querySelector(selectors: string): HTMLElement | null;
  querySelectorAll(selectors: string): NodeListOf<HTMLElement>;
}

// Event handlers here are on elements, so their target is one (with the fields they use).
interface EventTarget {
  closest(selectors: string): HTMLElement | null;
  readonly tagName: string;
  src: string;
  type: string;
  dataset: DOMStringMap;
}

// Form controls found by selector, and what the app keeps on its own elements.
interface HTMLElement {
  disabled?: boolean;
  placeholder?: string;
  _clear?: () => void;
  _held?: boolean;
}

// Errors from api() carry the answer's status and data.
interface Error {
  status?: number;
  offline?: boolean;
  data?: any;
  api?: boolean;
}

interface Window {
  applyTheme: (...args: any[]) => void;
  saveLangPref: (...args: any[]) => void;
  webkitAudioContext: typeof AudioContext;
  // Google's Cast library (cast.js) and Chrome.
  chrome: any;
  __onGCastApiAvailable: (ok: boolean) => void;
}

declare const cast: any;
declare const chrome: any;
