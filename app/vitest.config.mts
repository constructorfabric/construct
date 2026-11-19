import { defineConfig } from 'vitest/config';

export default defineConfig({
  test: {
    // The kit ships style sheets that Node cannot load; the bundler step turns them into nothing.
    server: { deps: { inline: ['@gears-frontx/ui-kit'] } },
    css: { modules: { classNameStrategy: 'non-scoped' } },
  },
});
