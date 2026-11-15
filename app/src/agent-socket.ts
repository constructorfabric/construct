import net from 'node:net';
import path from 'node:path';
import { parseStatusReply } from './agent-status';
import type { AgentStatus } from './agent-status';

const socketDir = 'construct';
const socketName = 'agent.sock';
const replyDeadlineMs = 2000;
const maxReplyChars = 4096;

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

/**
 * One `status` call. The deadline is for the whole call, whatever the agent writes meanwhile,
 * and the reply is capped, so a wedged or chatty agent cannot keep the call or its memory.
 */
function askStatus(socketPath: string, deadlineMs: number): Promise<AgentStatus> {
  return new Promise((resolve, reject) => {
    let reply = '';
    const socket = net.createConnection(socketPath);
    const deadline = setTimeout(() => {
      socket.destroy(new Error('the agent did not answer in time'));
    }, deadlineMs);
    socket.setEncoding('utf8');
    socket.on('connect', () => {
      socket.write('status\n');
    });
    socket.on('data', (chunk: string) => {
      reply += chunk;
      if (reply.includes('\n')) {
        socket.destroy();
        try {
          resolve(parseStatusReply(reply.trim()));
        } catch (error) {
          reject(error instanceof Error ? error : new Error(String(error)));
        }
      } else if (reply.length > maxReplyChars) {
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

function isAgentAbsent(error: unknown): boolean {
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
  deadlineMs = replyDeadlineMs,
): Promise<AgentStatus> {
  try {
    return await askStatus(agentSocketPath(env, platform), deadlineMs);
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
