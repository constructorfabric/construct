import {
  Badge,
  CodeBlock,
  CodeBlockActions,
  CodeBlockContent,
  CodeBlockCopyButton,
  CodeBlockHeader,
  CodeBlockTitle,
  Link,
} from '@gears-frontx/ui-kit';
import { Children, createContext, isValidElement, memo, useContext, useMemo } from 'react';
import type { ComponentPropsWithoutRef, ReactNode } from 'react';
import ReactMarkdown from 'react-markdown';
import type { Components } from 'react-markdown';
import remarkGfm from 'remark-gfm';
import { highlight } from './highlight';
import styles from './markdown.module.css';
import { webAddress } from './web-address';

const plugins = [remarkGfm];

/** What the renderer needs from the app: where a link goes, and its words. */
interface Setup {
  readonly onOpenLink: (address: string) => void;
  readonly imageLabel: string;
}

const SetupContext = createContext<Setup>({ onOpenLink: () => undefined, imageLabel: '' });

/** Whether what is drawn sits inside a link: a link cannot hold another one. */
const InsideLink = createContext(false);

/** A link of an answer: a web address goes where the app sends it, any other is plain text. */
function AnswerLink({ href, children }: { readonly href?: string; readonly children?: ReactNode }) {
  const { onOpenLink } = useContext(SetupContext);
  const inside = useContext(InsideLink);
  const address = webAddress(href);
  if (address === null || inside) {
    return <span>{children}</span>;
  }
  return (
    <InsideLink value>
      <Link
        href={address}
        title={address}
        onClick={(event) => {
          // The window itself never navigates: the app opens the address.
          event.preventDefault();
          onOpenLink(address);
        }}
      >
        {children}
      </Link>
    </InsideLink>
  );
}

/** The text of a fenced block and the language its fence names, from the `code` element inside. */
function fencedCode(children: ReactNode): { text: string; language: string | null } {
  for (const child of Children.toArray(children)) {
    if (!isValidElement<ComponentPropsWithoutRef<'code'>>(child)) {
      continue;
    }
    const { className, children: content } = child.props;
    const text = Children.toArray(content)
      .filter((part) => typeof part === 'string')
      .join('');
    const language = /(?:^|\s)language-(\S+)/.exec(className ?? '')?.[1] ?? null;
    // The parser ends every block's text with a line break that is not part of the code.
    return { text: text.replace(/\n$/, ''), language };
  }
  return { text: '', language: null };
}

function Image({ src, alt }: { readonly src?: string | Blob; readonly alt?: string }) {
  const { imageLabel } = useContext(SetupContext);
  const address = webAddress(src);
  // The window loads no image: the reader sees that there was one, its description, and where it is.
  return (
    <span className={styles.image}>
      <Badge variant="secondary">{imageLabel}</Badge>
      {alt === undefined || alt === '' ? null : ` ${alt} `}
      {address === null ? null : <AnswerLink href={address}>{address}</AnswerLink>}
    </span>
  );
}

/** A fenced block. Its highlighting is worked out again only when its text or language changes. */
function CodeFence({ children }: { readonly children?: ReactNode }) {
  const { text, language } = fencedCode(children);
  const tokens = useMemo(() => highlight(text, language), [text, language]);
  return (
    <CodeBlock className={`not-typeset ${styles.codeBlock}`}>
      {language === null ? (
        <CodeBlockActions>
          <CodeBlockCopyButton value={text} />
        </CodeBlockActions>
      ) : (
        <CodeBlockHeader>
          <CodeBlockTitle>{language}</CodeBlockTitle>
          <CodeBlockActions>
            <CodeBlockCopyButton value={text} />
          </CodeBlockActions>
        </CodeBlockHeader>
      )}
      <CodeBlockContent wrap>{tokens === null ? text : <span>{tokens}</span>}</CodeBlockContent>
    </CodeBlock>
  );
}

const components: Components = {
  a: ({ href, children }) => <AnswerLink href={href}>{children}</AnswerLink>,
  img: ({ src, alt }) => <Image src={src} alt={alt} />,
  // A fenced block comes here as a `pre` around a `code`; its text goes to the kit's block.
  pre: ({ children }) => <CodeFence>{children}</CodeFence>,
  table: ({ children }) => (
    <div className="typeset-scroll">
      <table>{children}</table>
    </div>
  ),
};

/**
 * An answer as Markdown. Raw HTML in it is text, not markup: there is no plugin that would let it
 * through. It is parsed again as the answer grows, and an unclosed code fence is a code block
 * that is still being written. The renderer knows nothing of the app around it: a link is handed
 * to `onOpenLink`, which must open an address in the system browser, never in this window.
 */
export const Markdown = memo(function Markdown({
  text,
  onOpenLink,
  imageLabel,
}: {
  readonly text: string;
  readonly onOpenLink: (address: string) => void;
  /** The word for an image that is not shown. */
  readonly imageLabel: string;
}): ReactNode {
  const setup = useMemo(() => ({ onOpenLink, imageLabel }), [onOpenLink, imageLabel]);
  return (
    <SetupContext value={setup}>
      <div className={`typeset typeset-chat ${styles.markdown}`}>
        <ReactMarkdown remarkPlugins={plugins} components={components}>
          {text}
        </ReactMarkdown>
      </div>
    </SetupContext>
  );
});
