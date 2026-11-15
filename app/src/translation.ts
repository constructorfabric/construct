import english from './en.json';

type Key = keyof typeof english;

/** The window's texts, English only. A placeholder with no value stays as written. */
export function t(key: Key, variables: Readonly<Record<string, string | number>> = {}): string {
  return english[key].replaceAll(/\{\{(\w+)\}\}/g, (placeholder, name: string) =>
    String(variables[name] ?? placeholder),
  );
}
