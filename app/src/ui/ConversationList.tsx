import {
  AlertDialog,
  AlertDialogAction,
  AlertDialogCancel,
  AlertDialogContent,
  AlertDialogFooter,
  AlertDialogHeader,
  AlertDialogTitle,
  AlertDialogTrigger,
  Button,
  SidebarGroup,
  SidebarGroupContent,
  Text,
} from '@gears-frontx/ui-kit';
import { Plus, Trash2 } from 'lucide-react';
import type { ReactNode } from 'react';
import { t } from '../translation';
import styles from './app.module.css';
import type { Chat } from './useChat';

/** The sidebar part of the chat page: New chat, the conversations, and Delete all. */
export function ConversationList({ chat }: { readonly chat: Chat }): ReactNode {
  const { conversations, active, running, notice } = chat.state;

  return (
    <SidebarGroup aria-label={t('ConversationList.Title')}>
      <SidebarGroupContent className={styles.conversations}>
        <Button
          id="chat-new"
          variant="outline"
          icon={<Plus />}
          disabled={running}
          onClick={chat.newChat}
        >
          {t('ConversationList.New')}
        </Button>
        <ul id="chat-conversations" className={styles.conversationItems}>
          {conversations.map((conversation) => (
            <li key={conversation.id} className={styles.conversationRow}>
              <Button
                className={styles.conversationItem}
                variant="ghost"
                aria-current={conversation.id === active ? 'true' : undefined}
                disabled={running}
                onClick={() => {
                  chat.open(conversation.id);
                }}
              >
                <span className={styles.conversationTitle}>
                  {conversation.title === '' ? t('ConversationList.Untitled') : conversation.title}
                </span>
              </Button>
              <Button
                variant="ghost"
                size="sm"
                icon={<Trash2 />}
                aria-label={t('ConversationList.Delete', { title: conversation.title })}
                disabled={running}
                onClick={() => {
                  chat.remove(conversation.id);
                }}
              />
            </li>
          ))}
        </ul>
        {notice === null ? null : (
          <Text id="chat-notice" tone="muted" role="alert" className={styles.notice}>
            {t('ConversationList.Notice', { reason: notice })}
          </Text>
        )}
        {conversations.length === 0 ? null : (
          <AlertDialog>
            <AlertDialogTrigger
              render={
                <Button id="chat-delete-all" variant="ghost" size="sm" disabled={running}>
                  {t('ConversationList.DeleteAll')}
                </Button>
              }
            />
            <AlertDialogContent size="sm">
              <AlertDialogHeader>
                <AlertDialogTitle>{t('ConversationList.ConfirmTitle')}</AlertDialogTitle>
              </AlertDialogHeader>
              <AlertDialogFooter>
                <AlertDialogCancel id="chat-delete-all-cancel">
                  {t('ConversationList.ConfirmNo')}
                </AlertDialogCancel>
                <AlertDialogAction
                  id="chat-delete-all-confirm"
                  variant="destructive"
                  disabled={running}
                  onClick={chat.removeAll}
                >
                  {t('ConversationList.ConfirmYes')}
                </AlertDialogAction>
              </AlertDialogFooter>
            </AlertDialogContent>
          </AlertDialog>
        )}
      </SidebarGroupContent>
    </SidebarGroup>
  );
}
