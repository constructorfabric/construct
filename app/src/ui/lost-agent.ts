/** The text of a failed bridge call: its own message, or that the window lost the agent. */
export function lostAgent(error: unknown): string {
  return error instanceof Error ? error.message : 'the window lost the agent';
}
