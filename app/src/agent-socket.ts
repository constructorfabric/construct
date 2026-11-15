import net from 'node:net';
import path from 'node:path';
import { parseStatusReply } from './agent-status';
import type { AgentStatus } from './agent-status';

const socketDir = 'construct';
const socketName = 'agent.sock';

/** Where the agent listens: `$XDG_RUNTIME_DIR` on Linux, `$TMPDIR` on macOS. */
export function agentSocketPath(env: NodeJS.ProcessEnv, platform: NodeJS.Platform): string {
  if (platform === 'win32') {
    throw new Error('the agent socket is not implemented on Windows yet');
  }
  const variable = platform === 'darwin' ? 'TMPDIR' : 'XDG_RUNTIME_DIR';
  const dir = env[variable];
  if (dir === undefined || !path.isAbsolute(dir)) {
    throw new Error(`${variable} is not set`);
  }
  return path.join(dir, socketDir, socketName);
}

/** How long one call waits, and how much of a reply it keeps. */
export interface CallLimits {
  readonly deadlineMs: number;
  readonly maxReplyChars: number;
}

export const statusLimits: CallLimits = { deadlineMs: 2000, maxReplyChars: 4096 };
/** The agent gives a model test 15 s; the app waits a little longer, so the agent's answer wins. */
export const testLimits: CallLimits = { deadlineMs: 20_000, maxReplyChars: 4096 };
export const settingsLimits: CallLimits = { deadlineMs: 5000, maxReplyChars: 4096 };

/**
 * One call: a command line goes out, one reply line comes back. The deadline is for the whole
 * call, whatever the agent writes meanwhile, and the reply is capped, so a wedged or chatty
 * agent cannot keep the call or its memory. The line may hold an API key, so no error repeats it.
 */
function askAgent(socketPath: string, line: string, limits: CallLimits): Promise<string> {
  return new Promise((resolve, reject) => {
    let reply = '';
    const socket = net.createConnection(socketPath);
    const deadline = setTimeout(() => {
      socket.destroy(new Error('the agent did not answer in time'));
    }, limits.deadlineMs);
    socket.setEncoding('utf8');
    socket.on('connect', () => {
      socket.write(`${line}\n`);
    });
    socket.on('data', (chunk: string) => {
      reply += chunk;
      if (reply.includes('\n')) {
        socket.destroy();
        resolve(reply.trim());
      } else if (reply.length > limits.maxReplyChars) {
        socket.destroy(new Error('the agent reply is too long'));
      }
    });
    socket.on('error', reject);
    socket.on('close', () => {
      clearTimeout(deadline);
      reject(new Error('the agent closed the socket without an answer'));
    });
  });
}

/** Sends `line` to the agent and gives its one reply line; fails when nobody answers. */
export function callAgent(
  env: NodeJS.ProcessEnv,
  platform: NodeJS.Platform,
  line: string,
  limits: CallLimits,
): Promise<string> {
  return askAgent(agentSocketPath(env, platform), line, limits);
}

export function isAgentAbsent(error: unknown): boolean {
  return (
    error instanceof Error &&
    'code' in error &&
    (error.code === 'ENOENT' || error.code === 'ECONNREFUSED')
  );
}

/**
 * Asks the agent for its status. `stopped` means the socket says nobody is there; any other
 * failure (no runtime directory, a bad or late reply) is `unknown`, not a claim about the agent.
 */
export async function readAgentStatus(
  env: NodeJS.ProcessEnv,
  platform: NodeJS.Platform,
  deadlineMs = statusLimits.deadlineMs,
): Promise<AgentStatus> {
  try {
    const line = await callAgent(env, platform, 'status', { ...statusLimits, deadlineMs });
    return parseStatusReply(line);
  } catch (error) {
    const reason = error instanceof Error ? error.message : String(error);
    return {
      state: isAgentAbsent(error) ? 'stopped' : 'unknown',
      reason,
      // Windows has no agent socket yet and the app cannot register the agent there.
      startAtLogin: platform === 'win32' ? 'not_supported' : 'unknown',
    };
  }
}
