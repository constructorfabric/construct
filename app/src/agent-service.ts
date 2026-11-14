import { execFile } from 'node:child_process';
import path from 'node:path';

const agentName = process.platform === 'win32' ? 'construct-agent.exe' : 'construct-agent';

/** Set to `1` to register the agent from a development run too (see the README). */
export const REGISTER_IN_DEV_VARIABLE = 'CONSTRUCT_AGENT_REGISTER';

export type AgentLocation = {
  readonly packaged: boolean;
  /** `process.resourcesPath`: where Electron Forge's `extraResource` puts the agent. */
  readonly resourcesPath: string;
  /** `app.getAppPath()`: the `app/` directory in a development run. */
  readonly appPath: string;
};

/** The agent that ships with the app, or the `cargo` output in a development run. */
export function agentBinaryPath(location: AgentLocation): string {
  if (location.packaged) {
    return path.join(location.resourcesPath, agentName);
  }
  return path.join(location.appPath, '..', 'target', 'release', agentName);
}

/**
 * A development build would leave a service that points into the build tree, so it
 * registers only when asked to; an installed app always does.
 */
export function shouldRegisterAgent(packaged: boolean, env: NodeJS.ProcessEnv): boolean {
  return packaged || env[REGISTER_IN_DEV_VARIABLE] === '1';
}

/**
 * Asks the agent to register itself as a login service. `ensure-service` does nothing when
 * the service is registered for this very binary, so this is safe at every start.
 */
export function ensureAgentService(binary: string, env: NodeJS.ProcessEnv): Promise<string> {
  return new Promise((resolve, reject) => {
    execFile(binary, ['ensure-service'], { env, timeout: 30_000 }, (error, stdout, stderr) => {
      if (error) {
        reject(new Error(`ensure-service failed: ${stderr.trim() || error.message}`));
      } else {
        resolve(stdout.trim());
      }
    });
  });
}
