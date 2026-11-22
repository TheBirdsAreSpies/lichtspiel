/// <reference types="vite/client" />

declare module '*.vue' {
  import type { DefineComponent } from 'vue';
  const component: DefineComponent<object, object, unknown>;
  export default component;
}

// A language pack of the usual web app (web/lang/<code>.js), made into a module by vite.config.ts.
declare module 'virtual:lang/*' {
  const pack: { name: string, strings: Record<string, string>, patterns: [RegExp, string][] };
  export default pack;
}
