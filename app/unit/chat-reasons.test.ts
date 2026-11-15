import { describe, expect, it } from 'vitest';
import english from '../src/en.json';
import { reasonText } from '../src/ui/chat-reasons';

const catalogue: Readonly<Record<string, string>> = english;

// Every reason the agent gives (and the two the app adds), with what the window says of it.
const reasons: readonly (readonly [string, string, boolean])[] = [
  ['key refused', 'ChatReason.KeyRefused', true],
  ['model not found', 'ChatReason.ModelNotFound', true],
  ['unreachable', 'ChatReason.Unreachable', true],
  ['timeout', 'ChatReason.Timeout', false],
  ['no model', 'ChatReason.NoModel', true],
  ['busy', 'ChatReason.Busy', false],
  ['too long', 'ChatReason.TooLong', false],
  ['cut', 'ChatReason.Cut', false],
  ['invalid request', 'ChatReason.Invalid', false],
  ['empty message', 'ChatReason.Empty', false],
  ['unknown conversation', 'ChatReason.UnknownConversation', false],
  ['request too large', 'ChatReason.RequestTooLarge', false],
  ['agent not running', 'ChatReason.AgentAbsent', false],
];

describe('reasonText', () => {
  it.each(reasons)('says %s with %s', (reason, key, settings) => {
    const text = reasonText(reason);
    expect(text.key).toBe(key);
    expect(text.settings).toBe(settings);
    expect(catalogue[text.key], `${text.key} is in en.json`).toBeDefined();
  });

  it('reads the status of a provider error', () => {
    const text = reasonText('error 503');
    expect(text).toEqual({ key: 'ChatReason.Status', values: { status: '503' }, settings: false });
    expect(catalogue[text.key]).toContain('{{status}}');
  });

  it('shows a reason it does not know, and never mistakes a property name for one', () => {
    for (const reason of ['something new', 'constructor', 'error 5', 'error 5030']) {
      const text = reasonText(reason);
      expect(text.key).toBe('ChatReason.Other');
      expect(text.values).toEqual({ reason });
      expect(catalogue[text.key]).toContain('{{reason}}');
    }
  });
});
