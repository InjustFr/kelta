// Flat ESLint config for the JS/TS workspace (ui, packages/*). Generated code is ignored.
import js from '@eslint/js';
import { defineConfig } from 'eslint/config';
import svelte from 'eslint-plugin-svelte';
import globals from 'globals';
import ts from 'typescript-eslint';

const noTimers = {
  'no-restricted-globals': [
    'error',
    { name: 'setInterval', message: 'No intervals (ARCHITECTURE §13): use event-armed one-shots.' },
  ],
  'no-restricted-properties': [
    'error',
    { object: 'window', property: 'setInterval', message: 'No intervals (ARCHITECTURE §13).' },
    { object: 'globalThis', property: 'setInterval', message: 'No intervals (ARCHITECTURE §13).' },
  ],
};

export default defineConfig(
  {
    ignores: [
      '**/dist/**',
      '**/node_modules/**',
      '**/test-results/**',
      '**/playwright-report/**',
      'ui/src/lib/gen/**',
      'target/**',
      'apps/**',
      'crates/**',
      'bench/**',
      'fixtures/**',
      'xtask/**',
    ],
  },
  js.configs.recommended,
  ts.configs.recommended,
  svelte.configs.recommended,
  {
    languageOptions: {
      ecmaVersion: 2024,
      sourceType: 'module',
      globals: { ...globals.browser, ...globals.node },
    },
    rules: {
      ...noTimers,
      '@typescript-eslint/no-unused-vars': ['error', { argsIgnorePattern: '^_', varsIgnorePattern: '^_' }],
      '@typescript-eslint/consistent-type-imports': ['error', { fixStyle: 'inline-type-imports' }],
      'no-console': ['warn', { allow: ['warn', 'error'] }],
    },
  },
  {
    files: ['**/*.svelte', '**/*.svelte.ts', '**/*.svelte.js'],
    languageOptions: {
      parserOptions: {
        parser: ts.parser,
        extraFileExtensions: ['.svelte'],
      },
    },
  },
  {
    // Global declarations need `import()` types.
    files: ['**/*.d.ts'],
    rules: { '@typescript-eslint/consistent-type-imports': 'off' },
  },
  {
    files: ['**/*.test.ts', '**/tests/**/*.ts'],
    rules: { 'no-console': 'off' },
  },
);
