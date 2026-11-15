import { execFileSync } from 'node:child_process';
import { chmodSync, existsSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import path from 'node:path';
import { _electron as electron, expect, test } from '@playwright/test';
import { agentBinaryPath, shouldRegisterAgent } from '../src/agent-service';
import { screenshotBothThemes } from './screenshots';

const appDir = path.resolve(__dirname, '..');
const agentBinary = path.resolve(appDir, '..', 'target', 'release', 'construct-agent');
const launchArgs = ['--no-sandbox'];

function stringEnv(extra: Record<string, string>): Record<string, string> {
  const merged: Record<string, string> = {};
  for (const [key, value] of Object.entries(process.env)) {
    if (value !== undefined) {
      merged[key] = value;
    }
  }
  return { ...merged, ...extra };
}

test.describe('where the agent is', () => {
  test('an installed app uses the binary in its resources', () => {
    expect(
      agentBinaryPath({ packaged: true, resourcesPath: '/opt/App/resources', appPath: '/x' }),
    ).toBe(path.join('/opt/App/resources', 'construct-agent'));
  });

  test('a development run uses the cargo output', () => {
    expect(
      agentBinaryPath({ packaged: false, resourcesPath: '/x', appPath: '/src/construct/app' }),
    ).toBe(path.join('/src/construct', 'target', 'release', 'construct-agent'));
  });

  test('only an installed app, or a development run that asks, registers the agent', () => {
    expect(shouldRegisterAgent(true, {})).toBe(true);
    expect(shouldRegisterAgent(false, {})).toBe(false);
    expect(shouldRegisterAgent(false, { CONSTRUCT_AGENT_REGISTER: '1' })).toBe(true);
  });
});

type Sandbox = {
  readonly configHome: string;
  readonly systemctlLog: string;
  readonly runtimeDir: string;
  readonly env: Record<string, string>;
};

// The unit directory is a throw-away `XDG_CONFIG_HOME`, and `systemctl` is a script that only
// writes down its arguments, so no run touches the real user services.
function makeSandbox(startAgentOnEnable = false): Sandbox {
  const root = mkdtempSync(path.join(tmpdir(), 'cb-reg-'));
  const bin = path.join(root, 'bin');
  const systemctlLog = path.join(root, 'systemctl.log');
  const configHome = path.join(root, 'config');
  const runtimeDir = path.join(root, 'run');
  execFileSync('mkdir', [bin, runtimeDir]);
  const fake = path.join(bin, 'systemctl');
  // Like systemd, `enable --now` starts the agent as a process of its own, not a child of the caller
  // and without the caller's open files (Playwright waits for the app's pipes to close, and
  // Electron leaves many of them open in its children).
  const start = startAgentOnEnable
    ? `case "$*" in *"enable --now"*) ( for f in /proc/self/fd/*; do n=\${f##*/}; [ "$n" -gt 2 ] && eval "exec $n>&-"; done; exec setsid "${agentBinary}" run ) </dev/null >/dev/null 2>&1 & ;; esac\n`
    : '';
  writeFileSync(fake, `#!/bin/bash\necho "$@" >> "${systemctlLog}"\n${start}`);
  chmodSync(fake, 0o755);
  return {
    configHome,
    systemctlLog,
    runtimeDir,
    env: stringEnv({
      PATH: `${bin}:${process.env.PATH ?? ''}`,
      XDG_CONFIG_HOME: configHome,
      XDG_RUNTIME_DIR: runtimeDir,
    }),
  };
}

const unitFile = (sandbox: Sandbox) =>
  path.join(sandbox.configHome, 'systemd', 'user', 'construct-agent.service');

test('a development run that asks registers the agent with this repo’s binary', async () => {
  const sandbox = makeSandbox();
  const userData = mkdtempSync(path.join(tmpdir(), 'cb-data-'));
  const app = await electron.launch({
    args: [appDir, ...launchArgs, `--user-data-dir=${userData}`],
    env: { ...sandbox.env, CONSTRUCT_AGENT_REGISTER: '1' },
  });
  try {
    await expect.poll(() => existsSync(unitFile(sandbox))).toBe(true);
    expect(readFileSync(unitFile(sandbox), 'utf8')).toContain(`ExecStart="${agentBinary}" run`);
    expect(readFileSync(sandbox.systemctlLog, 'utf8')).toContain(
      '--user enable --now construct-agent.service',
    );
  } finally {
    await app.close();
    rmSync(path.dirname(sandbox.systemctlLog), { recursive: true, force: true });
    rmSync(userData, { recursive: true, force: true });
  }
});

test('a plain development run registers nothing', async () => {
  const sandbox = makeSandbox();
  const userData = mkdtempSync(path.join(tmpdir(), 'cb-data-'));
  const app = await electron.launch({
    args: [appDir, ...launchArgs, `--user-data-dir=${userData}`],
    env: sandbox.env,
  });
  try {
    await expect((await app.firstWindow()).locator('#agent-status')).toBeVisible();
    expect(existsSync(unitFile(sandbox))).toBe(false);
    expect(existsSync(sandbox.systemctlLog)).toBe(false);
  } finally {
    await app.close();
    rmSync(path.dirname(sandbox.systemctlLog), { recursive: true, force: true });
    rmSync(userData, { recursive: true, force: true });
  }
});

test('the app registers the agent, shows it, and leaves it running when it quits', async () => {
  const sandbox = makeSandbox(true);
  const userData = mkdtempSync(path.join(tmpdir(), 'cb-data-'));
  const status = () =>
    JSON.parse(execFileSync(agentBinary, ['status'], { env: sandbox.env, encoding: 'utf8' })) as {
      pid: number;
    };
  const app = await electron.launch({
    args: [appDir, ...launchArgs, `--user-data-dir=${userData}`],
    env: { ...sandbox.env, CONSTRUCT_AGENT_REGISTER: '1' },
  });
  let agentPid: number | undefined;
  try {
    // The app's registration made the (fake) service manager start the agent.
    await expect.poll(() => existsSync(unitFile(sandbox))).toBe(true);
    await expect(() => {
      agentPid = status().pid;
    }).toPass({ timeout: 10_000 });

    const page = await app.firstWindow();
    const block = page.locator('#start-at-login');
    await expect(block).toHaveText('registered', { timeout: 20_000 });
    await expect(block).toHaveAttribute('data-state', 'registered');
    await screenshotBothThemes(page, 'start-at-login-registered');

    await app.close();
    expect(status().pid).toBe(agentPid);
  } finally {
    if (agentPid !== undefined) {
      process.kill(agentPid, 'SIGTERM');
    }
    rmSync(path.dirname(sandbox.systemctlLog), { recursive: true, force: true });
    rmSync(userData, { recursive: true, force: true });
  }
});
