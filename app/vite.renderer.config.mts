import { readFileSync } from 'node:fs';
import react from '@vitejs/plugin-react';
import { defineConfig } from 'vite';
import type { Plugin } from 'vite';

// The page's policy is strict. The dev server injects CSS through a `<style>` element and talks to
// the page over a websocket, so `npm start` gets the looser policy in dev-csp.txt; the built page
// keeps the strict one from index.html.
const devPolicy = readFileSync(new URL('dev-csp.txt', import.meta.url), 'utf8').trim();

const devContentSecurityPolicy = {
  name: 'dev-content-security-policy',
  apply: 'serve',
  transformIndexHtml: (html) =>
    html.replace(/(http-equiv="Content-Security-Policy"\s+content=")[^"]*"/, `$1${devPolicy}"`),
} satisfies Plugin;

export default defineConfig({ plugins: [react(), devContentSecurityPolicy] });
