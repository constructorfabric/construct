import eslint from '@eslint/js';
import tseslint from 'typescript-eslint';

export default tseslint.config(
  { ignores: ['.vite/**', 'out/**', 'test-results/**', 'playwright-report/**'] },
  eslint.configs.recommended,
  tseslint.configs.recommended,
);
