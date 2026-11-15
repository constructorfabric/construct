export interface ReasonText {
  /** The translation key of the message. */
  readonly key: string;
  readonly values?: Readonly<Record<string, string | number>>;
  /** Whether Settings is where the user can fix it. */
  readonly settings: boolean;
}

const known = new Map<string, ReasonText>(
  Object.entries({
    'key refused': { key: 'ChatReason.KeyRefused', settings: true },
    'model not found': { key: 'ChatReason.ModelNotFound', settings: true },
    unreachable: { key: 'ChatReason.Unreachable', settings: true },
    timeout: { key: 'ChatReason.Timeout', settings: false },
    'no model': { key: 'ChatReason.NoModel', settings: true },
    busy: { key: 'ChatReason.Busy', settings: false },
    'too long': { key: 'ChatReason.TooLong', settings: false },
    cut: { key: 'ChatReason.Cut', settings: false },
    'invalid request': { key: 'ChatReason.Invalid', settings: false },
    'empty message': { key: 'ChatReason.Empty', settings: false },
    'unknown conversation': { key: 'ChatReason.UnknownConversation', settings: false },
    'request too large': { key: 'ChatReason.RequestTooLarge', settings: false },
    'agent not running': { key: 'ChatReason.AgentAbsent', settings: false },
  }),
);

const providerStatus = /^error (\d{3})$/;

/**
 * The message for a reason of the agent. The reasons are fixed texts of the agent or the app, so
 * one that is not known is still safe to show.
 */
export function reasonText(reason: string): ReasonText {
  const found = known.get(reason);
  if (found !== undefined) {
    return found;
  }
  const status = providerStatus.exec(reason);
  if (status?.[1] !== undefined) {
    return { key: 'ChatReason.Status', values: { status: status[1] }, settings: false };
  }
  return { key: 'ChatReason.Other', values: { reason }, settings: false };
}
