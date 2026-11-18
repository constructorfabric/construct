import { describe, expect, it } from 'vitest';
import type { MemoryFact } from '../src/memory-protocol';
import { buildTree, fieldName, firstText } from '../src/ui/memory-tree';

const fact = (
  id: string,
  entity_kind: MemoryFact['entity_kind'],
  property: string,
  value: MemoryFact['value'],
): MemoryFact => ({ fact_id: id, entity_kind, property, value, stored_at: '2026-11-18T10:30:00Z' });

describe('fieldName', () => {
  it('writes the key with spaces and a capital', () => {
    expect(fieldName('answer_language')).toBe('Answer language');
    expect(fieldName('skills')).toBe('Skills');
  });

  it('takes the text of en.json where the key reads wrong', () => {
    expect(fieldName('orcid_id')).toBe('ORCID iD');
  });
});

describe('buildTree', () => {
  it('orders the kinds, groups the facts of a property and keeps their order', () => {
    const tree = buildTree([
      fact('1', 'preference', 'answer_language', 'English'),
      fact('2', 'skill', 'skills', { name: 'Rust' }),
      fact('3', 'identity', 'full_name', 'Ada'),
      fact('4', 'skill', 'skills', { name: 'Go' }),
    ]);
    expect(tree.map((kind) => kind.kind)).toEqual(['identity', 'skill', 'preference']);
    const skills = tree[1]?.properties[0];
    expect(skills?.name).toBe('Skills');
    expect(skills?.facts.map((f) => f.id)).toEqual(['2', '4']);
  });

  it('turns an object value into named fields and keeps nested objects as fields', () => {
    const [kind] = buildTree([
      fact('1', 'skill', 'skills', { name: 'Rust', level_detail: { years: '5' } }),
    ]);
    expect(kind?.properties[0]?.facts[0]?.value).toEqual({
      kind: 'fields',
      fields: [
        { name: 'Name', value: { kind: 'text', text: 'Rust' } },
        {
          name: 'Level detail',
          value: {
            kind: 'fields',
            fields: [{ name: 'Years', value: { kind: 'text', text: '5' } }],
          },
        },
      ],
    });
  });

  it('turns an array inside an object value into a list', () => {
    const [kind] = buildTree([
      fact('1', 'preference', 'work_schedule', { rest_days: ['Sat', 'Sun'] }),
    ]);
    expect(kind?.properties[0]?.facts[0]?.value).toEqual({
      kind: 'fields',
      fields: [
        {
          name: 'Rest days',
          value: {
            kind: 'list',
            items: [
              { kind: 'text', text: 'Sat' },
              { kind: 'text', text: 'Sun' },
            ],
          },
        },
      ],
    });
  });

  it('has no kind without facts', () => {
    expect(buildTree([])).toEqual([]);
  });
});

describe('firstText', () => {
  it('finds the first text inside fields and lists', () => {
    expect(
      firstText({
        kind: 'fields',
        fields: [
          { name: 'A', value: { kind: 'list', items: [] } },
          { name: 'B', value: { kind: 'list', items: [{ kind: 'text', text: 'deep' }] } },
        ],
      }),
    ).toBe('deep');
  });
});
