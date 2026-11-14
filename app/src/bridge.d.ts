import type { ConstructAgentApi } from './agent-status';

declare global {
  // An interface, not a type: it has to merge with the DOM's `Window`.
  interface Window {
    readonly constructAgent: ConstructAgentApi;
  }
}
