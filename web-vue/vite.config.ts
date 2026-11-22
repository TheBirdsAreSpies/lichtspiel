import { readFileSync } from 'node:fs';
import { fileURLToPath } from 'node:url';
import { defineConfig, type Plugin } from 'vite';
import vue from '@vitejs/plugin-vue';

// The server serves the build at /vue/ (src/server/http.rs, vue_file). `npm run dev` runs Vite on its own
// port and passes the API on to a Lichtspiel server (LICHTSPIEL_URL, default http://localhost:8484).
const server = process.env.LICHTSPIEL_URL || 'http://localhost:8484';
const proxy = Object.fromEntries(['/api', '/library', '/hubs', '/video', '/:/', '/identity', '/status', '/playlists', '/favicon.ico']
  .map(p => [p, { target: server, changeOrigin: true }]));

// The language packs are the usual web app's (web/lang/<code>.js), so both apps share one set of
// translations. Each file calls I18N.add(code, name, {strings, patterns}); it is run here, at build time,
// and turned into a module (`import de from 'virtual:lang/de'`), so the page needs no eval.
const LANGS = ['de'];
function langPacks(): Plugin {
  const prefix = 'virtual:lang/';
  return {
    name: 'lichtspiel-lang',
    resolveId: id => (id.startsWith(prefix) ? '\0' + id : null),
    load(id) {
      if (!id.startsWith('\0' + prefix)) return null;
      const code = id.slice(prefix.length + 1);
      if (!LANGS.includes(code)) throw new Error('No language pack ' + code);
      const file = fileURLToPath(new URL(`../web/lang/${code}.js`, import.meta.url));
      this.addWatchFile(file);
      let pack: { name: string, strings: Record<string, string>, patterns: [RegExp, string][] } | null = null;
      const I18N = { add: (_c: string, name: string, p: { strings?: Record<string, string>, patterns?: [RegExp, string][] }) => { pack = { name, strings: p.strings || {}, patterns: p.patterns || [] }; } };
      new Function('I18N', readFileSync(file, 'utf8'))(I18N);
      if (!pack) throw new Error(`web/lang/${code}.js did not call I18N.add`);
      const p = pack as { name: string, strings: Record<string, string>, patterns: [RegExp, string][] };
      const patterns = p.patterns.map(([re, rep]) => `[new RegExp(${JSON.stringify(re.source)}, ${JSON.stringify(re.flags)}), ${JSON.stringify(rep)}]`);
      return `export default { name: ${JSON.stringify(p.name)}, strings: ${JSON.stringify(p.strings)}, patterns: [${patterns.join(',')}] };`;
    },
  };
}

export default defineConfig({
  base: '/vue/',
  plugins: [vue(), langPacks()],
  server: { proxy },
  build: { outDir: 'dist', emptyOutDir: true, sourcemap: false },
});
