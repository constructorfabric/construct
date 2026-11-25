import { defineConfig } from '@playwright/test';

export default defineConfig({
  testDir: 'tests',
  timeout: 60_000,
  workers: 1,
  // On the CI a runner that is busy with other jobs makes the timing tests of the chat (a poll of five seconds,
  // a count of reads in a window) fail now and then, and a run takes 13 minutes. A failed test is run again,
  // twice at most: one that passes on the second try is reported as flaky, not hidden, and one that fails every
  // time still fails the job. A developer's Mac does not retry, so a failure there shows at once.
  retries: process.env.CI ? 2 : 0,
});
