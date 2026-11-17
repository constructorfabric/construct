import {
  Alert,
  Bubble,
  BubbleContent,
  Button,
  Empty,
  EmptyActions,
  EmptyHeader,
  EmptyTitle,
  InputGroup,
  InputGroupAddon,
  InputGroupTextarea,
  Marker,
  MarkerContent,
  Message,
  MessageContent,
  MessageScroller,
  MessageScrollerButton,
  MessageScrollerContent,
  MessageScrollerItem,
  MessageScrollerProvider,
  MessageScrollerViewport,
  Spinner,
  useMessageScroller,
  useMessageScrollerScrollable,
} from '@gears-frontx/ui-kit';
import { ArrowUp, Square } from 'lucide-react';
import { useEffect, useRef, useState } from 'react';
import type { KeyboardEvent, ReactNode, RefObject } from 'react';
import { t } from '../translation';
import styles from './app.module.css';
import { reasonText } from './chat-reasons';
import type { Failure } from './chat-state';
import { Markdown } from './markdown';
import type { Chat } from './useChat';

/** Opens an address in the system browser; the window itself never leaves its page. */
function openLink(address: string): void {
  void window.constructAgent.openLink(address);
}

/** How far from the bottom the thread still counts as "at the bottom", in pixels. */
const bottomSlackPx = 48;

const exampleKeys = ['EmptyChat.ExampleKnow', 'EmptyChat.ExampleRust'] as const;

export function ChatPage({
  chat,
  modelSet,
  profileAvailable,
  onOpenSettings,
}: {
  readonly chat: Chat;
  /** Whether the agent has a model; `false` only when it says so. */
  readonly modelSet: boolean;
  /** Whether the Construct gear can give the profile; `false` only when the agent says so. */
  readonly profileAvailable: boolean;
  readonly onOpenSettings: () => void;
}): ReactNode {
  const { messages, running, streaming, failure, loading, loadError, active } = chat.state;
  const last = messages.at(-1);
  const noModelFailure = failure?.kind === 'reason' && failure.reason === 'no model';
  const empty = messages.length === 0 && !loading && loadError === null && failure === null;
  // What follows the messages in the thread; the last of them is what the thread follows down.
  const trailing: { readonly id: string; readonly node: ReactNode }[] = [];
  // A tool line that shimmers already says that the agent works.
  if (running && !streaming && last?.role !== 'tool') {
    trailing.push({
      id: 'waiting',
      node: <Spinner id="chat-waiting" aria-label={t('Thread.Waiting')} />,
    });
  }
  if (loading) {
    trailing.push({ id: 'loading', node: <Spinner aria-label={t('ChatPage.Loading')} /> });
  }
  if (loadError !== null) {
    trailing.push({
      id: 'load-error',
      node: <Alert variant="destructive">{t('ChatPage.LoadFailed', { reason: loadError })}</Alert>,
    });
  }
  if (failure !== null) {
    trailing.push({
      id: 'failure',
      node: (
        <FailureMessage failure={failure} onRetry={chat.retry} onOpenSettings={onOpenSettings} />
      ),
    });
  }
  const lastId = trailing.at(-1)?.id ?? `message-${messages.length - 1}`;
  // Whether the reader is at the bottom: only then does a growing answer pull the thread down.
  const atBottom = useRef(true);

  return (
    <div className={styles.chat}>
      {/* A new conversation is a new transcript, so it gets a new scroller. */}
      <MessageScrollerProvider key={active} scrollEdgeThreshold={bottomSlackPx}>
        <MessageScroller className={styles.thread}>
          <MessageScrollerViewport
            id="chat-thread"
            role="log"
            aria-label={t('Thread.Label')}
            onScroll={(event) => {
              const element = event.currentTarget;
              atBottom.current =
                element.scrollHeight - element.scrollTop - element.clientHeight <= bottomSlackPx;
            }}
          >
            <MessageScrollerContent className={styles.threadContent}>
              {messages.map((message, index) => (
                <MessageScrollerItem
                  // Messages are only ever added at the end or grown, so the position is their
                  // identity.
                  key={index}
                  messageId={`message-${index}`}
                  scrollAnchor={`message-${index}` === lastId}
                >
                  {message.role === 'tool' ? (
                    // Only the newest line of a running answer moves; the rest are still.
                    <ToolLine
                      text={message.text}
                      moving={running && index === messages.length - 1}
                    />
                  ) : (
                    <Message
                      align={message.role === 'user' ? 'end' : 'start'}
                      data-role={message.role}
                    >
                      <MessageContent>
                        <Bubble
                          variant={message.role === 'user' ? 'outline' : 'ghost'}
                          align={message.role === 'user' ? 'end' : 'start'}
                        >
                          {message.role === 'user' ? (
                            <BubbleContent className={styles.messageText}>
                              {message.text}
                            </BubbleContent>
                          ) : (
                            <BubbleContent className={styles.answerText}>
                              <Markdown
                                text={message.text}
                                onOpenLink={openLink}
                                imageLabel={t('Markdown.Image')}
                              />
                            </BubbleContent>
                          )}
                        </Bubble>
                      </MessageContent>
                    </Message>
                  )}
                </MessageScrollerItem>
              ))}
              {trailing.map((item) => (
                <MessageScrollerItem
                  key={item.id}
                  messageId={item.id}
                  scrollAnchor={item.id === lastId}
                >
                  {item.node}
                </MessageScrollerItem>
              ))}
              {empty ? <EmptyChat onExample={chat.send} /> : null}
            </MessageScrollerContent>
          </MessageScrollerViewport>
          <JumpToEnd />
          <FollowNewest
            lastId={lastId}
            size={last?.text.length ?? 0}
            lastIsUser={last?.role === 'user'}
            atBottom={atBottom}
          />
        </MessageScroller>
      </MessageScrollerProvider>
      <div className={styles.composerArea}>
        {profileAvailable ? null : (
          <Alert id="chat-no-profile" role="status">
            {t('NoProfile.Text')}
          </Alert>
        )}
        {!modelSet && !noModelFailure ? (
          <Alert id="chat-no-model">
            <span className={styles.alertContent}>
              {t('NoModel.Text')}
              <Button id="chat-open-settings" variant="outline" size="sm" onClick={onOpenSettings}>
                {t('NoModel.OpenSettings')}
              </Button>
            </span>
          </Alert>
        ) : null}
        <Composer chat={chat} />
      </div>
    </div>
  );
}

/** The quiet status line of one tool call; the text comes from the agent. */
function ToolLine({
  text,
  moving,
}: {
  readonly text: string;
  readonly moving: boolean;
}): ReactNode {
  return (
    <Marker role="status" data-role="tool">
      <MarkerContent className={moving ? 'shimmer' : undefined}>{text}</MarkerContent>
    </Marker>
  );
}

/** The button that brings a reader who scrolled up back to the newest message. */
function JumpToEnd(): ReactNode {
  const { end } = useMessageScrollerScrollable();
  return end ? <MessageScrollerButton direction="end" aria-label={t('Thread.ToEnd')} /> : null;
}

/**
 * Takes the thread to the end when a message is added or the answer grows, as long as the reader
 * is at the bottom; one who scrolled up to read is left where they are. A new message lands at the
 * top of the viewport, not the bottom, so this is also what shows it. It renders nothing.
 */
function FollowNewest({
  lastId,
  size,
  lastIsUser,
  atBottom,
}: {
  readonly lastId: string;
  /** The length of the last message: it changes as the answer grows. */
  readonly size: number;
  readonly lastIsUser: boolean;
  readonly atBottom: RefObject<boolean>;
}): ReactNode {
  const { scrollToEnd } = useMessageScroller();
  const previous = useRef<string | null>(null);
  useEffect(() => {
    const newMessage = lastId !== previous.current;
    if (newMessage) {
      // A question the user has just sent is always shown, and so is a thread that was just opened.
      if (previous.current === null || lastIsUser) {
        atBottom.current = true;
      }
      previous.current = lastId;
    }
    if (!atBottom.current) {
      return;
    }
    if (!newMessage) {
      // The answer only grew: follow at once, or a fast stream leaves the thread behind.
      scrollToEnd({ behavior: 'instant' });
      return;
    }
    // A new message: two frames land after the scroller's own adjustment has painted.
    let frame = requestAnimationFrame(() => {
      frame = requestAnimationFrame(() => {
        scrollToEnd({ behavior: 'instant' });
      });
    });
    return () => {
      cancelAnimationFrame(frame);
    };
  }, [lastId, size, lastIsUser, atBottom, scrollToEnd]);
  return null;
}

function EmptyChat({ onExample }: { readonly onExample: (message: string) => void }): ReactNode {
  return (
    <Empty id="chat-empty" className={styles.empty}>
      <EmptyHeader>
        <EmptyTitle render={<h1 />}>{t('EmptyChat.Title')}</EmptyTitle>
      </EmptyHeader>
      <EmptyActions>
        {exampleKeys.map((key) => (
          <Button
            key={key}
            variant="outline"
            onClick={() => {
              onExample(t(key));
            }}
          >
            {t(key)}
          </Button>
        ))}
      </EmptyActions>
    </Empty>
  );
}

function FailureMessage({
  failure,
  onRetry,
  onOpenSettings,
}: {
  readonly failure: Failure;
  readonly onRetry: () => void;
  readonly onOpenSettings: () => void;
}): ReactNode {
  const retry = (
    <Button id="chat-retry" variant="outline" size="sm" onClick={onRetry}>
      {t('FailureMessage.Retry')}
    </Button>
  );
  if (failure.kind === 'stopped') {
    return (
      <Alert id="chat-failure">
        <span className={styles.alertContent}>
          {t('FailureMessage.Stopped')}
          {retry}
        </span>
      </Alert>
    );
  }
  const text = reasonText(failure.reason);
  return (
    <Alert id="chat-failure" variant="destructive">
      <span className={styles.alertContent}>
        {/* The reasons are fixed texts, and each of them has its key in the table. */}
        {t(text.key as Parameters<typeof t>[0], text.values)}
        {text.settings ? (
          <Button id="chat-failure-settings" variant="outline" size="sm" onClick={onOpenSettings}>
            {t('FailureMessage.OpenSettings')}
          </Button>
        ) : null}
        {retry}
      </span>
    </Alert>
  );
}

function Composer({ chat }: { readonly chat: Chat }): ReactNode {
  const [draft, setDraft] = useState('');
  const input = useRef<HTMLTextAreaElement>(null);
  const { running, active } = chat.state;

  // Typing is for a thread that is open: a new chat, a chosen conversation, or a finished answer.
  useEffect(() => {
    if (!running) {
      input.current?.focus();
    }
  }, [running, active]);

  const send = (): void => {
    // The draft stays when the chat refuses it (a conversation is still being read).
    if (chat.send(draft)) {
      setDraft('');
    }
  };

  const onKeyDown = (event: KeyboardEvent): void => {
    // Enter sends, Shift+Enter is a new line, and Enter that confirms an input method does neither.
    if (event.key === 'Enter' && !event.shiftKey && !event.nativeEvent.isComposing) {
      event.preventDefault();
      send();
    }
  };

  return (
    <InputGroup>
      <InputGroupTextarea
        id="chat-input"
        ref={input}
        rows={1}
        className={styles.composerInput}
        value={draft}
        onChange={(event) => {
          setDraft(event.target.value);
        }}
        onKeyDown={onKeyDown}
        placeholder={t('Composer.Placeholder')}
        aria-label={t('Composer.Message')}
      />
      <InputGroupAddon align="block-end">
        <div className={styles.composerActions}>
          {running ? (
            <Button
              id="chat-stop-button"
              variant="outline"
              icon={<Square />}
              aria-label={t('Composer.Stop')}
              onClick={chat.stop}
            />
          ) : null}
          <Button
            id="chat-send-button"
            icon={<ArrowUp />}
            aria-label={t('Composer.Send')}
            disabled={running || draft.trim() === ''}
            onClick={send}
          />
        </div>
      </InputGroupAddon>
    </InputGroup>
  );
}
