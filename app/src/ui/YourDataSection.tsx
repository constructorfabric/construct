import {
  Alert,
  AlertDialog,
  AlertDialogAction,
  AlertDialogCancel,
  AlertDialogContent,
  AlertDialogDescription,
  AlertDialogFooter,
  AlertDialogHeader,
  AlertDialogTitle,
  AlertDialogTrigger,
  Button,
  Spinner,
} from '@gears-frontx/ui-kit';
import { useState } from 'react';
import type { ReactNode } from 'react';
import { t } from '../translation';
import { reasonText } from './chat-reasons';
import styles from './app.module.css';
import { ControlRow } from './components/ControlRow';
import { Section } from './components/Section';
import { OutcomeMessage } from './ModelSettingsForm';
import type { Chat } from './useChat';
import type { Erasing } from './useErasing';

/**
 * The "Your data" section of Settings: delete all chats, and delete everything the agent and
 * the gear keep about the person. The erasure itself is kept above the page (see `useErasing`),
 * so leaving Settings neither loses the wait nor its result.
 */
export function YourDataSection({
  chat,
  erasure,
}: {
  readonly chat: Chat;
  readonly erasure: Erasing;
}): ReactNode {
  const [chatsError, setChatsError] = useState<string | null>(null);
  const busy = erasure.state.kind === 'asking' || erasure.state.kind === 'waiting';
  const { conversations } = chat.state;

  const deleteChats = async (): Promise<void> => {
    const reason = await chat.removeAll();
    if (reason === null) {
      setChatsError(null);
      return;
    }
    // Delete all stops a running answer first; the agent still refuses when that answer will not end.
    // The table's keys are all in en.json (checked by the unit test of `reasonText`).
    const text = reasonText(reason);
    setChatsError(t(text.key as Parameters<typeof t>[0], text.values));
  };

  return (
    <Section
      titleId="your-data-title"
      title={t('YourData.Title')}
      description={t('YourData.Hint')}
      after={
        <>
          {chatsError === null ? null : (
            <OutcomeMessage id="chats-error" outcome={{ kind: 'error', text: chatsError }} />
          )}
          {busy ? <Spinner id="erase-running" label={t('YourData.Erasing')} /> : null}
          {erasure.state.kind === 'done' ? (
            <Alert id="erase-done" className={styles.outcome} role="status">
              {t('YourData.Done')}
            </Alert>
          ) : null}
          {erasure.state.kind === 'failed' ? (
            <OutcomeMessage
              id="erase-error"
              outcome={{ kind: 'error', text: erasure.state.reason }}
            />
          ) : null}
        </>
      }
    >
      <ControlRow
        id="your-data-chats"
        label={t('YourData.Chats')}
        description={t('YourData.ChatsHint')}
      >
        <AlertDialog>
          <AlertDialogTrigger
            render={
              <Button
                id="chat-delete-all"
                variant="outline"
                aria-labelledby="your-data-chats-label"
                disabled={busy || conversations.length === 0}
              >
                {t('YourData.Chats')}
              </Button>
            }
          />
          <AlertDialogContent size="sm">
            <AlertDialogHeader>
              <AlertDialogTitle>{t('YourData.ChatsConfirmTitle')}</AlertDialogTitle>
            </AlertDialogHeader>
            <AlertDialogFooter>
              <AlertDialogCancel id="chat-delete-all-cancel">
                {t('YourData.Cancel')}
              </AlertDialogCancel>
              <AlertDialogAction
                id="chat-delete-all-confirm"
                variant="destructive"
                onClick={() => void deleteChats()}
              >
                {t('YourData.ChatsConfirmYes')}
              </AlertDialogAction>
            </AlertDialogFooter>
          </AlertDialogContent>
        </AlertDialog>
      </ControlRow>
      <ControlRow
        id="your-data-everything"
        label={t('YourData.Everything')}
        description={t('YourData.EverythingHint')}
      >
        <AlertDialog>
          <AlertDialogTrigger
            render={
              <Button
                id="erase-everything"
                variant="destructive"
                aria-labelledby="your-data-everything-label"
                disabled={busy}
              >
                {t('YourData.Everything')}
              </Button>
            }
          />
          <AlertDialogContent size="sm">
            <AlertDialogHeader>
              <AlertDialogTitle>{t('YourData.EverythingConfirmTitle')}</AlertDialogTitle>
              <AlertDialogDescription id="erase-everything-body">
                {t('YourData.EverythingConfirmBody')}
              </AlertDialogDescription>
            </AlertDialogHeader>
            <AlertDialogFooter>
              <AlertDialogCancel id="erase-everything-cancel">
                {t('YourData.Cancel')}
              </AlertDialogCancel>
              <AlertDialogAction
                id="erase-everything-confirm"
                variant="destructive"
                onClick={() => void erasure.start()}
              >
                {t('YourData.EverythingConfirmYes')}
              </AlertDialogAction>
            </AlertDialogFooter>
          </AlertDialogContent>
        </AlertDialog>
      </ControlRow>
    </Section>
  );
}
