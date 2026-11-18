import english from '../en.json';
import { entityKinds } from '../memory-protocol';
import type { EntityKind, JsonValue, MemoryFact } from '../memory-protocol';

/** A value of a fact as the page draws it: a text, an object's fields, or a list. */
export type TreeValue =
  | { readonly kind: 'text'; readonly text: string }
  | { readonly kind: 'fields'; readonly fields: readonly TreeField[] }
  | { readonly kind: 'list'; readonly items: readonly TreeValue[] };

export interface TreeField {
  readonly name: string;
  readonly value: TreeValue;
}

export interface TreeFact {
  readonly id: string;
  readonly storedAt: Date;
  readonly value: TreeValue;
}

/** The facts of one property of the profile. */
export interface TreeProperty {
  readonly key: string;
  readonly name: string;
  readonly facts: readonly TreeFact[];
}

/** The properties of one category of the profile. */
export interface TreeKind {
  readonly kind: EntityKind;
  readonly properties: readonly TreeProperty[];
}

/**
 * The plain name of a key: the key with spaces and a capital, or the text `en.json` gives it
 * under `MemoryField.<key>` where that reads wrong.
 */
export function fieldName(key: string): string {
  const override = `MemoryField.${key}`;
  if (Object.hasOwn(english, override)) {
    return english[override as keyof typeof english];
  }
  const spaced = key.replaceAll(/[_-]+/g, ' ').trim();
  return spaced === '' ? key : spaced.charAt(0).toUpperCase() + spaced.slice(1);
}

function treeValue(value: JsonValue): TreeValue {
  if (Array.isArray(value)) {
    return { kind: 'list', items: value.map((item) => treeValue(item)) };
  }
  if (typeof value === 'object' && value !== null) {
    return {
      kind: 'fields',
      fields: Object.entries(value).map(([key, field]) => ({
        name: fieldName(key),
        value: treeValue(field),
      })),
    };
  }
  return { kind: 'text', text: value === null ? '' : String(value) };
}

/**
 * The facts as a tree: the categories in the profile's own order, then each property in the order
 * its first fact comes, then its facts as the gear lists them (the newest first).
 */
export function buildTree(facts: readonly MemoryFact[]): TreeKind[] {
  return entityKinds.flatMap((kind) => {
    const properties = new Map<string, TreeFact[]>();
    for (const fact of facts) {
      if (fact.entity_kind !== kind) {
        continue;
      }
      const list = properties.get(fact.property) ?? [];
      list.push({
        id: fact.fact_id,
        storedAt: new Date(fact.stored_at),
        value: treeValue(fact.value),
      });
      properties.set(fact.property, list);
    }
    if (properties.size === 0) {
      return [];
    }
    return [
      {
        kind,
        properties: [...properties].map(([key, list]) => ({
          key,
          name: fieldName(key),
          facts: list,
        })),
      },
    ];
  });
}

/** The first text a value holds, for the row of a value that is collapsed; empty when none. */
export function firstText(value: TreeValue): string {
  if (value.kind === 'text') {
    return value.text;
  }
  const parts = value.kind === 'fields' ? value.fields.map((field) => field.value) : value.items;
  for (const part of parts) {
    const text = firstText(part);
    if (text !== '') {
      return text;
    }
  }
  return '';
}
