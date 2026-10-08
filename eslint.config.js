// @ts-check
// One flat config for every workspace package (ESLint looks it up from each
// linted file). `npm run lint` runs it per package with --max-warnings 0.
import js from '@eslint/js';
import prettier from 'eslint-config-prettier';
import { defineConfig, globalIgnores } from 'eslint/config';
import tsdoc from 'eslint-plugin-tsdoc';
import globals from 'globals';
import tseslint from 'typescript-eslint';

/** Node built-in modules, with and without the `node:` prefix. */
const NODE_BUILTINS = [
  'assert',
  'buffer',
  'child_process',
  'crypto',
  'events',
  'fs',
  'fs/promises',
  'http',
  'https',
  'net',
  'os',
  'path',
  'process',
  'stream',
  'url',
  'util',
  'worker_threads',
  'zlib',
];

export default defineConfig(
  globalIgnores([
    '**/dist/',
    '**/docs-out/',
    '**/coverage/',
    'examples/',
    'tools/',
    'crates/',
    'target/',
    'packages/*/test-d/',
  ]),
  js.configs.recommended,
  tseslint.configs.strictTypeChecked,
  tseslint.configs.stylisticTypeChecked,
  {
    languageOptions: {
      parserOptions: { projectService: true, tsconfigRootDir: import.meta.dirname },
    },
    plugins: { tsdoc },
    rules: {
      // `any` is allowed only at typed boundaries, each with a justified
      // inline disable (CONTRIBUTING.md, "Lint exceptions").
      '@typescript-eslint/no-explicit-any': 'error',
      '@typescript-eslint/explicit-module-boundary-types': 'error',
      '@typescript-eslint/consistent-type-imports': 'error',
      'tsdoc/syntax': 'error',
      '@typescript-eslint/no-unused-vars': ['error', { argsIgnorePattern: '^_' }],
    },
  },
  {
    // The API package and the guest shims run in webviews only: browser
    // globals, and no Node module may be imported (G2 "no Node imports").
    files: ['packages/api/**/*.{ts,tsx}', 'packages/guest-shims/**/*.ts'],
    ignores: ['**/*.config.*', '**/scripts/**'],
    languageOptions: { globals: { ...globals.browser } },
    rules: {
      'no-restricted-imports': [
        'error',
        {
          paths: NODE_BUILTINS.flatMap((name) => [
            { name, message: 'browser-only package: no Node modules' },
            { name: `node:${name}`, message: 'browser-only package: no Node modules' },
          ]),
          patterns: [{ group: ['node:*'], message: 'browser-only package: no Node modules' }],
        },
      ],
      'no-restricted-globals': [
        'error',
        { name: 'process', message: 'browser-only package: no Node globals' },
        { name: 'Buffer', message: 'browser-only package: no Node globals' },
        { name: 'require', message: 'browser-only package: no Node globals' },
        { name: '__dirname', message: 'browser-only package: no Node globals' },
      ],
    },
  },
  {
    files: ['packages/cli/**/*.ts'],
    languageOptions: { globals: { ...globals.node } },
  },
  {
    files: ['**/*.test.ts', '**/test-d/**'],
    rules: {
      '@typescript-eslint/no-non-null-assertion': 'off',
      // Tests simulate Tauri rejecting with plain strings and objects.
      '@typescript-eslint/only-throw-error': 'off',
      '@typescript-eslint/prefer-promise-reject-errors': 'off',
      '@typescript-eslint/unbound-method': 'off',
      '@typescript-eslint/no-deprecated': 'off',
    },
  },
  {
    files: ['**/*.{js,mjs}', '**/*.config.{js,mjs,ts}'],
    extends: [tseslint.configs.disableTypeChecked],
    languageOptions: { globals: { ...globals.node } },
  },
  prettier,
);
