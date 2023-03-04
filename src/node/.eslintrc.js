// Copyright 2018-2021 Signal Messenger, LLC
// SPDX-License-Identifier: AGPL-3.0-only

const rules = {
  'comma-dangle': [
    'error',
    {
      arrays: 'always-multiline',
      objects: 'always-multiline',
      imports: 'always-multiline',
      exports: 'always-multiline',
      functions: 'never',
    },
  ],

  // prevents us from accidentally checking in exclusive tests (`.only`):
  'mocha/no-exclusive-tests': 'error',

  // it helps readability to put public API at top,
  'no-use-before-define': 'off',

  // useful for unused or internal fields
  'no-underscore-dangle': 'off',

  // though we have a logger, we still remap console to log to disk
  'no-console': 'error',

  // consistently place operators at end of line except ternaries
  'operator-linebreak': [
    'error',
    'after',
    { overrides: { '?': 'ignore', ':': 'ignore' } },
  ],

  quotes: [
    'error',
    'single',
    { avoidEscape: true, allowTemplateLiterals: false },
  ],

  'no-continue': 'off',
  'no-bitwise': 'off',
  'no-plusplus': 'off',

  // Prettier overrides:
  'arrow-parens': 'off',
  'function-paren-newline': 'off',

  // We prefer named exports
  'import/prefer-default-export': 'off',

  'no-restricted-syntax': [
    'error',
    {
      selector: 'ForInStatement',
      message:
        'for..in loops iterate over the entire prototype chain, which is virtually never what you want. Use Object.{keys,values,entries}, and iterate over the resulting array.',
    },
    {
      selector: 'LabeledStatement',
      message:
        'Labels are a form of GOTO; using them makes code confusing and hard to maintain and understand.',
    },
    {
      selector: 'WithStatement',
      message:
        '`with` is disallowed in strict mode because it makes code impossible to predict and optimize.',
    },
  ],
  curly: 'error',

  'prefer-template': 'error',

  // Things present in our existing code that we may want to be stricter about in the future.
  eqeqeq: 'off',
  // The RingRTC singleton is used in VideoSupport; maybe we should untangle this.
  'import/no-cycle': 'off',
};

const typescriptRules = {
  ...rules,

  '@typescript-eslint/array-type': ['error', { default: 'generic' }],

  // Overrides recommended by typescript-eslint
  //   https://github.com/typescript-eslint/typescript-eslint/releases/tag/v4.0.0
  '@typescript-eslint/no-redeclare': 'error',
  '@typescript-eslint/no-shadow': ['error', { ignoreOnInitialization: true }],
  '@typescript-eslint/no-useless-constructor': ['error'],
  'no-shadow': 'off',
  'no-useless-constructor': 'off',

  // useful for unused parameters
  '@typescript-eslint/no-unused-vars': ['error', { argsIgnorePattern: '^_' }],

  // Upgrade from a warning
  '@typescript-eslint/explicit-module-boundary-types': 'error',

  // Already enforced by TypeScript
  'consistent-return': 'off',

  // Things present in our existing code that we may want to be stricter about in the future.
  '@typescript-eslint/no-explicit-any': 'off',
  '@typescript-eslint/no-unsafe-assignment': 'off',
  '@typescript-eslint/no-unsafe-argument': 'off',
  '@typescript-eslint/no-unsafe-member-access': 'off',
  '@typescript-eslint/no-unsafe-call': 'off',
  '@typescript-eslint/restrict-template-expressions': 'off',
  '@typescript-eslint/no-use-before-define': 'off',
};

module.exports = {
  root: true,
  env: {
    node: true,
    es2018: true,
  },
  settings: {
    'import/core-modules': ['electron'],
  },

  extends: ['eslint:recommended', 'prettier'],

  plugins: ['import', 'mocha', 'more'],

  overrides: [
    {
      files: ['**/*.ts'],
      parser: '@typescript-eslint/parser',
      parserOptions: {
        project: 'tsconfig.json',
        sourceType: 'module',
      },
      plugins: ['@typescript-eslint'],
      extends: [
        'plugin:@typescript-eslint/recommended',
        'plugin:@typescript-eslint/recommended-requiring-type-checking',
      ],
      rules: typescriptRules,
    },
  ],

  rules,
};
