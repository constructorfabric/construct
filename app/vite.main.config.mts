import { defineConfig } from 'vite';

// https://vitejs.dev/config
export default defineConfig({
  define: {
    // The address the Glasshouse page opens first can be given when the app is packaged, for an
    // environment of one's own: `CONSTRUCT_GLASSHOUSE_URL=https://… npm run make`. Without it the
    // public address of `src/glasshouse.ts` opens. The address is in the package, so it is not a place for a secret.
    __GLASSHOUSE_PACKAGED_START_ADDRESS__: JSON.stringify(
      process.env['CONSTRUCT_GLASSHOUSE_URL'] ?? '',
    ),
  },
});
