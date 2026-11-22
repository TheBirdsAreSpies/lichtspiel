// Languages. As in the usual web app (web/i18n.js), the English text is the key: t('Sign in') gives the text
// of the chosen language, or the English text when the pack has none; {name} is filled from vars.
// The packs are the usual web app's (web/lang/<code>.js, see vite.config.ts): new German text goes there.
import { ref } from 'vue';
import de from 'virtual:lang/de';

type Pack = { name: string, strings: Record<string, string>, patterns: [RegExp, string][] };
export const packs: Record<string, Pack> = { en: { name: 'English', strings: {}, patterns: [] }, de };

function pick(): string {
  try { const saved = localStorage.getItem('lang'); if (saved && packs[saved]) return saved; } catch { /* storage blocked */ }
  for (const l of navigator.languages || [navigator.language || 'en']) {
    const c = String(l).toLowerCase().split('-')[0];
    if (packs[c]) return c;
  }
  return 'en';
}

/** The language in use; templates that call t() are drawn again when it changes. */
export const lang = ref(pick());
document.documentElement.lang = lang.value;

export function setLang(code: string) {
  if (!packs[code]) return;
  lang.value = code;
  document.documentElement.lang = code;
  try { localStorage.setItem('lang', code); } catch { /* storage blocked */ }
}

export function t(s: string, vars?: Record<string, string | number>): string {
  const p = packs[lang.value];
  let out = s;
  if (p && lang.value !== 'en') {
    if (Object.prototype.hasOwnProperty.call(p.strings, s)) out = p.strings[s];
    else for (const [re, rep] of p.patterns) { if (re.test(s)) { out = s.replace(re, rep); break; } }
  }
  return vars ? out.replace(/\{(\w+)\}/g, (m, k: string) => (k in vars ? String(vars[k]) : m)) : out;
}
