export function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === 'object' && value !== null;
}

/** A number that can count something: finite and not negative. */
export function isCount(value: unknown): value is number {
  return typeof value === 'number' && Number.isFinite(value) && value >= 0;
}

/** One reply line of the agent as a JSON object; anything else is an error. */
export function parseLine(line: string): Record<string, unknown> {
  const reply: unknown = JSON.parse(line);
  if (!isRecord(reply)) {
    throw new Error('the agent reply is not a JSON object');
  }
  return reply;
}

/** The text of an `{"error": "..."}` reply, which the agent keeps fixed; otherwise `null`. */
export function replyError(reply: Record<string, unknown>): string | null {
  return typeof reply.error === 'string' ? reply.error : null;
}
