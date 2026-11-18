import { execFileSync, spawn } from 'node:child_process';
import type { ChildProcess } from 'node:child_process';
import { closeSync, mkdirSync, mkdtempSync, openSync, readFileSync, rmSync } from 'node:fs';
import net from 'node:net';
import { tmpdir } from 'node:os';
import path from 'node:path';
import { agentSocketPath } from '../src/agent-socket';
import { _electron as electron, expect, test as base } from '@playwright/test';
import type { ElectronApplication, Page } from '@playwright/test';

export { expect };

export const appDir = path.resolve(__dirname, '..');
export const agentBinary = path.resolve(appDir, '..', 'target', 'release', 'construct-agent');

// Under a virtual display on a VPS the Chromium sandbox cannot start, and `--no-sandbox` turns it
// off for every process. `sandbox: true` on the window still keeps Node out of the renderer, which
// the tests check; the OS-level sandbox is not exercised by this run.
const launchArgs = ['--no-sandbox'];

export interface Session {
  /** The agent that runs now; `restartAgent` replaces it. */
  readonly agent: ChildProcess;
  readonly appArgs: readonly string[];
  readonly env: Record<string, string>;
  /** The agent's `XDG_CONFIG_HOME`, `CODEX_HOME` and `CLAUDE_CONFIG_DIR`. */
  readonly configHome: string;
  readonly codexHome: string;
  readonly claudeHome: string;
  /** Where the agent keeps its records, `records/<tool>/<session>.jsonl`. */
  readonly recordsDir: string;
  /** Everything the agent has written to its output and error streams, over all its runs. */
  readonly agentLog: () => string;
  readonly stopAgent: () => Promise<void>;
  readonly startAgent: () => Promise<void>;
}

function stringEnv(extra: Record<string, string>): Record<string, string> {
  const merged: Record<string, string> = {};
  for (const [key, value] of Object.entries(process.env)) {
    if (value !== undefined) {
      merged[key] = value;
    }
  }
  return { ...merged, ...extra };
}

/** An app window that listens for policy violations from the first page load on. */
export interface OpenedApp {
  readonly app: ElectronApplication;
  readonly page: Page;
  /** What the main process has written to its output and error streams. */
  readonly output: () => string;
}

export async function openApp(session: Session): Promise<OpenedApp> {
  const app = await electron.launch({ args: [...session.appArgs], env: session.env });
  return { app, ...(await prepareWindow(app)) };
}

/**
 * Opens the app, runs `check` on its window and closes the app, whatever the check does. Any
 * `securitypolicyviolation` of the page fails the check.
 */
export async function checkInApp(
  session: Session,
  check: (page: Page) => Promise<void>,
): Promise<void> {
  const { app, page } = await openApp(session);
  try {
    await page.getByRole('button', { name: 'Settings', exact: true }).waitFor();
    await check(page);
    await expectNoPolicyViolations(page);
  } finally {
    await app.close();
  }
}

/** Stops the agent and starts it again, as a restart of the machine does. */
export async function restartAgent(session: Session): Promise<void> {
  await session.stopAgent();
  await session.startAgent();
}

async function prepareWindow(
  app: ElectronApplication,
): Promise<Pick<OpenedApp, 'page' | 'output'>> {
  let output = '';
  for (const stream of [app.process().stdout, app.process().stderr]) {
    stream?.on('data', (chunk: Buffer) => {
      output += chunk.toString();
    });
  }
  const page = await app.firstWindow();
  // The first window, reloaded under a listener for policy violations: any `securitypolicyviolation`
  // event of the page fails the test that raised it.
  await page.addInitScript(() => {
    const violations: string[] = [];
    Object.assign(window, { policyViolations: violations });
    document.addEventListener('securitypolicyviolation', (event) => {
      violations.push(`${event.violatedDirective} ${event.blockedURI}`);
    });
  });
  await page.reload();
  return { page, output: () => output };
}

/** Opens the Settings page, once the section with this heading is there. */
export async function openSettings(page: Page, heading: string): Promise<void> {
  await page.getByRole('button', { name: 'Settings', exact: true }).click();
  await expect(page.getByRole('heading', { name: heading })).toBeVisible();
}

export async function expectNoPolicyViolations(page: Page): Promise<void> {
  expect(await page.evaluate(() => Reflect.get(window, 'policyViolations') as string[])).toEqual(
    [],
  );
}

/** What this process writes to its output and error streams, from now until `stop`. */
export function captureTestOutput(): { text: () => string; stop: () => void } {
  let text = '';
  const [stdoutWrite, stderrWrite] = [process.stdout.write, process.stderr.write];
  const wrap = (original: typeof process.stdout.write): typeof process.stdout.write =>
    function (this: unknown, chunk: unknown, ...rest: unknown[]): boolean {
      text += String(chunk);
      return Reflect.apply(original, this, [chunk, ...rest]) as boolean;
    };
  process.stdout.write = wrap(process.stdout.write.bind(process.stdout));
  process.stderr.write = wrap(process.stderr.write.bind(process.stderr));
  return {
    text: () => text,
    stop: () => {
      process.stdout.write = stdoutWrite;
      process.stderr.write = stderrWrite;
    },
  };
}

/** Everything on the page that a person could read or a script could reach. */
export async function windowText(page: Page): Promise<string> {
  return page.evaluate(() => {
    const inputs = [...document.querySelectorAll('input')].map(
      (input) => `${input.value}|${input.placeholder}`,
    );
    return [
      document.documentElement.outerHTML,
      document.body.innerText,
      ...inputs,
      JSON.stringify({ ...localStorage }),
      JSON.stringify({ ...sessionStorage }),
    ].join('\n');
  });
}

/**
 * Stops the real agent, puts a stand-in on its socket that answers each request line with
 * `answer` (a fixed reply, or a function of the line), and runs `check` on a new window; any
 * `securitypolicyviolation` of the page fails the check.
 */
export async function withStandIn(
  session: Session,
  answer: string | ((line: string) => string | Promise<string>),
  check: (page: Page, app: ElectronApplication) => Promise<void>,
): Promise<void> {
  const socketPath = agentSocketPath(session.env, process.platform);
  session.agent.kill('SIGTERM');
  if (session.agent.exitCode === null && session.agent.signalCode === null) {
    await new Promise((resolve) => session.agent.once('exit', resolve));
  }
  mkdirSync(path.dirname(socketPath), { recursive: true });
  rmSync(socketPath, { force: true });
  const server = net.createServer((socket) => {
    socket.on('error', () => undefined);
    if (typeof answer === 'string') {
      socket.end(`${answer}\n`);
      return;
    }
    socket.setEncoding('utf8');
    socket.once('data', (data: string) => {
      void Promise.resolve(answer(data.trim())).then((reply) => {
        socket.end(`${reply}\n`);
      });
    });
  });
  await new Promise<void>((resolve) => server.listen(socketPath, resolve));
  const { app, page } = await openApp(session);
  try {
    await check(page, app);
    await expectNoPolicyViolations(page);
  } finally {
    await app.close();
    server.close();
  }
}

export const test = base.extend<{ session: Session; app: ElectronApplication; page: Page }>({
  // A real agent from this repo, with its socket in a private runtime directory and its settings
  // in a private home directory. Playwright wants an object pattern here, even when the fixture
  // needs nothing.
  // eslint-disable-next-line no-empty-pattern
  session: async ({}, use) => {
    const runtimeDir = mkdtempSync(path.join(tmpdir(), 'cb-run-'));
    const userDataDir = mkdtempSync(path.join(tmpdir(), 'cb-data-'));
    // The agent reads its settings and looks for its login service in the home directory: a
    // private one keeps a model or a service of the machine running the test out of the page
    // and its screenshots.
    const homeDir = mkdtempSync(path.join(tmpdir(), 'cb-home-'));
    // The agent also starts its PostgreSQL; its data goes to a throw-away directory.
    const dataHome = mkdtempSync(path.join(tmpdir(), 'cb-pg-'));
    const logDir = mkdtempSync(path.join(tmpdir(), 'cb-log-'));
    // The tools' session files are fake and private, whatever the real ones say on this machine.
    const codexHome = mkdtempSync(path.join(tmpdir(), 'cb-codex-'));
    const claudeHome = mkdtempSync(path.join(tmpdir(), 'cb-claude-'));
    const configHome = path.join(homeDir, '.config');
    const logPath = path.join(logDir, 'agent.log');
    const env = stringEnv({
      XDG_RUNTIME_DIR: runtimeDir,
      TMPDIR: runtimeDir,
      XDG_DATA_HOME: dataHome,
      HOME: homeDir,
      XDG_CONFIG_HOME: configHome,
      CODEX_HOME: codexHome,
      CLAUDE_CONFIG_DIR: claudeHome,
    });
    let agent: ChildProcess | null = null;

    const startAgent = async (): Promise<void> => {
      const log = openSync(logPath, 'a');
      agent = spawn(agentBinary, ['run'], { env, stdio: ['ignore', log, log] });
      closeSync(log);
      await expect(() => {
        execFileSync(agentBinary, ['status'], { env, stdio: 'ignore' });
      }).toPass({ timeout: 10_000 });
    };
    const stopAgent = async (): Promise<void> => {
      const running = agent;
      if (running === null) {
        return;
      }
      running.kill('SIGTERM');
      // Let the agent stop PostgreSQL before its directories go away.
      if (running.exitCode === null && running.signalCode === null) {
        await new Promise((resolve) => running.once('exit', resolve));
      }
    };

    try {
      await startAgent();
      await use({
        get agent() {
          if (agent === null) {
            throw new Error('the agent was never started');
          }
          return agent;
        },
        env,
        configHome,
        codexHome,
        claudeHome,
        recordsDir: path.join(dataHome, 'construct', 'records'),
        appArgs: [appDir, ...launchArgs, `--user-data-dir=${userDataDir}`],
        agentLog: () => readFileSync(logPath, 'utf8'),
        stopAgent,
        startAgent,
      });
    } finally {
      await stopAgent();
      for (const dir of [
        runtimeDir,
        userDataDir,
        homeDir,
        dataHome,
        logDir,
        codexHome,
        claudeHome,
      ]) {
        rmSync(dir, { recursive: true, force: true });
      }
    }
  },
  app: async ({ session }, use) => {
    const app = await electron.launch({ args: [...session.appArgs], env: session.env });
    try {
      await use(app);
    } finally {
      await app.close();
    }
  },
  page: async ({ app }, use) => {
    const { page } = await prepareWindow(app);
    await use(page);
    await expectNoPolicyViolations(page);
  },
});
