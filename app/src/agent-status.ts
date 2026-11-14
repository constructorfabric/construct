export type AgentStatus =
  | { readonly state: 'running'; readonly version: string; readonly pid: number }
  /** The socket answered "nobody here": no socket file, or nothing listening on it. */
  | { readonly state: 'stopped'; readonly reason: string }
  /** The app could not find out: a local failure, or an agent that answered wrongly. */
  | { readonly state: 'unknown'; readonly reason: string };

export type ConstructAgentApi = {
  readonly getStatus: () => Promise<AgentStatus>;
};

export const STATUS_CHANNEL = 'agent:status';

/** Turns the agent's JSON reply line into a status; anything else is an error. */
export function parseStatusReply(line: string): AgentStatus {
  const reply: unknown = JSON.parse(line);
  if (typeof reply !== 'object' || reply === null) {
    throw new Error('the agent reply is not a JSON object');
  }
  if (!('status' in reply) || reply.status !== 'running') {
    throw new Error(`the agent did not report running: ${line}`);
  }
  if (!('version' in reply) || typeof reply.version !== 'string') {
    throw new Error('the agent reply has no version');
  }
  if (!('pid' in reply) || typeof reply.pid !== 'number') {
    throw new Error('the agent reply has no pid');
  }
  return { state: 'running', version: reply.version, pid: reply.pid };
}
