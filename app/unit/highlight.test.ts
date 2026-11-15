import { describe, expect, it } from 'vitest';
import { highlight, highlightLimit } from '../src/ui/markdown/highlight';

describe('highlight', () => {
  it('turns the code of a known language into tokens', () => {
    expect(highlight('fn main() {}', 'rust')?.length).toBeGreaterThan(0);
  });

  it('gives nothing, and does not throw, for a language it does not know or none', () => {
    for (const language of ['mermaid', 'dockerfile', '__proto__', 'constructor', null]) {
      expect(highlight('graph TD', language), String(language)).toBeNull();
    }
  });

  it('gives nothing for a block over the limit', () => {
    expect(highlight('a'.repeat(highlightLimit + 1), 'rust')).toBeNull();
    expect(highlight('a'.repeat(highlightLimit), 'rust')).not.toBeNull();
  });
});
