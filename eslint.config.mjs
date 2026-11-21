// Lint for the web app (npm run lint). The scripts in web/ are plain scripts sharing one global scope,
// so names from the other files are checked by the type check (npm run typecheck), not here.
import js from '@eslint/js';
import globals from 'globals';

export default [
  { ignores: ['clients/**', 'target/**', 'vendor/**', 'plugins/**', 'node_modules/**'] },
  js.configs.recommended,
  {
    files: ['web/**/*.js'],
    languageOptions: { ecmaVersion: 'latest', sourceType: 'script', globals: { ...globals.browser } },
    rules: {
      'no-undef': 'off',
      // Top-level functions are used by the other scripts.
      'no-unused-vars': ['error', { vars: 'local', args: 'none', caughtErrors: 'none' }],
      'no-empty': ['error', { allowEmptyCatch: true }],
    },
  },
];
