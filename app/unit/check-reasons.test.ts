import { describe, expect, it } from 'vitest';
import { plainFailures } from '../src/agent-settings';
import type { TestCheck } from '../src/agent-settings';
import { checkLabel, describeCheck, withProgress } from '../src/ui/check-reasons';

describe('describeCheck', () => {
  it('says a check is running until its line comes', () => {
    expect(describeCheck(null)).toBe('checking');
  });

  it('has its own plain text for every way a check can end', () => {
    const ended: TestCheck[] = [
      { name: 'a', result: 'works' },
      { name: 'a', result: 'skipped' },
      { name: 'a', result: 'error', status: 502 },
      ...plainFailures.map((result): TestCheck => ({ name: 'a', result })),
    ];
    const texts = ended.map((check) => describeCheck(check));
    expect(texts.every((text) => text !== '' && !text.includes('{{'))).toBe(true);
    expect(new Set(texts).size).toBe(texts.length);
    expect(describeCheck({ name: 'a', result: 'error', status: 502 })).toContain('502');
    expect(describeCheck({ name: 'a', result: 'no tool call' })).toBe(
      'the model did not call the tool',
    );
  });
});

describe('checkLabel', () => {
  it('names the checks it knows and shows another as the agent sent it', () => {
    expect(checkLabel('answers')).toBe('Answers');
    expect(checkLabel('uses tools')).toBe('Uses tools');
    expect(checkLabel('reads files')).toBe('reads files');
  });
});

describe('withProgress', () => {
  it('opens the rows with the names and fills a row with its check line', () => {
    const opened = withProgress([], { event: 'checks', names: ['answers', 'uses tools'] });
    expect(opened).toEqual([
      { name: 'answers', check: null },
      { name: 'uses tools', check: null },
    ]);
    const check: TestCheck = { name: 'answers', result: 'works' };
    expect(withProgress(opened, { event: 'check', check })).toEqual([
      { name: 'answers', check },
      { name: 'uses tools', check: null },
    ]);
  });

  it('ignores a check line for a row that is not there', () => {
    const rows = [{ name: 'answers', check: null }];
    expect(
      withProgress(rows, { event: 'check', check: { name: 'other', result: 'works' } }),
    ).toEqual(rows);
  });
});
