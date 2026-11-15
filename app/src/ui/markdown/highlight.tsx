import { common, createLowlight } from 'lowlight';
import type { ReactNode } from 'react';
import styles from './markdown.module.css';

/** Above this many characters a block is shown as plain text: highlighting costs time with its size. */
export const highlightLimit = 20_000;

const lowlight = createLowlight(common);

type Node = ReturnType<typeof lowlight.highlight>['children'][number];

function draw(node: Node, key: number): ReactNode {
  if (node.type === 'text') {
    return node.value;
  }
  if (node.type !== 'element') {
    return null;
  }
  const classes = node.properties.className;
  const names = Array.isArray(classes)
    ? classes.map((name) => styles[String(name)]).filter((name) => name !== undefined)
    : [];
  return (
    <span key={key} className={names.join(' ')}>
      {node.children.map(draw)}
    </span>
  );
}

/**
 * The code as tokens, each a `span` with a class from this module's style sheet (a colour is a
 * class, never an inline style, which the window's policy forbids). `null` for a language the
 * highlighter does not know, for none, and for a block too long to be worth it.
 */
export function highlight(code: string, language: string | null): ReactNode[] | null {
  if (language === null || code.length > highlightLimit || !lowlight.registered(language)) {
    return null;
  }
  return lowlight.highlight(language, code).children.map(draw);
}
