import { SidebarGroup, SidebarGroupContent, SidebarMenu, Text } from '@gears-frontx/ui-kit';
import { MessageCircle, Plus, Trash2 } from 'lucide-react';
import type { ReactNode } from 'react';
import { t } from '../translation';
import styles from './app.module.css';
import { ListRow } from './components/ListRow';
import { MenuItem } from './components/MenuItem';
import type { Chat } from './useChat';

/** The sidebar part of the chat page: New chat and the conversations. */
export function ConversationList({ chat }: { readonly chat: Chat }): ReactNode {
  const { conversations, active, running, notice } = chat.state;

  return (
    <SidebarGroup aria-label={t('ConversationList.Title')}>
      <SidebarGroupContent className={styles.conversations}>
        <SidebarMenu>
          <MenuItem
            id="chat-new"
            icon={<Plus aria-hidden />}
            label={t('ConversationList.New')}
            disabled={running}
            onSelect={chat.newChat}
          />
        </SidebarMenu>
        <SidebarMenu id="chat-conversations">
          {conversations.map((conversation) => {
            const title =
              conversation.title === '' ? t('ConversationList.Untitled') : conversation.title;
            return (
              <ListRow
                key={conversation.id}
                icon={<MessageCircle aria-hidden />}
                title={title}
                active={conversation.id === active}
                disabled={running}
                action={{
                  label: t('ConversationList.Delete', { title }),
                  icon: <Trash2 aria-hidden />,
                  onSelect: () => {
                    chat.remove(conversation.id);
                  },
                }}
                onSelect={() => {
                  chat.open(conversation.id);
                }}
              />
            );
          })}
        </SidebarMenu>
        {notice === null ? null : (
          <Text id="chat-notice" tone="muted" role="alert" className={styles.notice}>
            {t('ConversationList.Notice', { reason: notice })}
          </Text>
        )}
      </SidebarGroupContent>
    </SidebarGroup>
  );
}
