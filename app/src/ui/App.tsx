import {
  Alert,
  Sidebar,
  SidebarContent,
  SidebarFooter,
  SidebarGroup,
  SidebarGroupContent,
  SidebarInset,
  SidebarMenu,
  SidebarMenuButton,
  SidebarMenuItem,
  SidebarProvider,
  StatusDot,
} from '@gears-frontx/ui-kit';
import { Activity, MessageSquare, Settings } from 'lucide-react';
import { useEffect, useState } from 'react';
import type { CSSProperties, ReactNode } from 'react';
import { t } from '../translation';
import styles from './app.module.css';
import { ChatPage } from './ChatPage';
import { ConversationList } from './ConversationList';
import { SettingsPage } from './SettingsPage';
import { tones } from './state-tones';
import { StatusPage } from './StatusPage';
import { useAgentStatus } from './useAgentStatus';
import { useChat } from './useChat';

type Page = 'chat' | 'status' | 'settings';

export function App(): ReactNode {
  // A page is a piece of state: the app has no addresses to navigate to.
  const [page, setPage] = useState<Page>('chat');
  const { status, refresh } = useAgentStatus();
  const state = status?.state ?? 'unknown';
  const chat = useChat(page === 'chat');
  // The model may have been set in Settings a moment ago; the chat page must not say otherwise.
  useEffect(() => {
    if (page === 'chat') {
      refresh();
    }
  }, [page, refresh]);
  // Only an agent that says so has no model; one that cannot be reached is not "without a model".
  const modelSet = status?.state !== 'running' || status.model !== 'not set';
  const [needsMove, setNeedsMove] = useState(false);
  useEffect(() => {
    void window.constructAgent.getNeedsMove().then(setNeedsMove);
  }, []);

  return (
    // `defaultOpen` keeps the sidebar from reading its cookie; it never collapses on its own, and
    // the Ctrl/Cmd+B shortcut writes the cookie without any visible effect.
    <SidebarProvider defaultOpen style={{ '--sidebar-width': '14rem' } as CSSProperties}>
      <Sidebar collapsible="none" role="navigation" aria-label={t('App.Navigation')}>
        <SidebarContent>
          <SidebarGroup>
            <SidebarGroupContent>
              <SidebarMenu>
                <SidebarMenuItem>
                  <SidebarMenuButton
                    isActive={page === 'chat'}
                    render={
                      <button
                        type="button"
                        aria-current={page === 'chat' ? 'page' : undefined}
                        onClick={() => setPage('chat')}
                      />
                    }
                  >
                    <MessageSquare aria-hidden />
                    <span>{t('App.Chat')}</span>
                  </SidebarMenuButton>
                </SidebarMenuItem>
                <SidebarMenuItem>
                  <SidebarMenuButton
                    isActive={page === 'status'}
                    render={
                      <button
                        type="button"
                        aria-current={page === 'status' ? 'page' : undefined}
                        onClick={() => setPage('status')}
                      />
                    }
                  >
                    <Activity aria-hidden />
                    <span>{t('App.Status')}</span>
                  </SidebarMenuButton>
                </SidebarMenuItem>
                <SidebarMenuItem>
                  <SidebarMenuButton
                    isActive={page === 'settings'}
                    render={
                      <button
                        type="button"
                        aria-current={page === 'settings' ? 'page' : undefined}
                        onClick={() => setPage('settings')}
                      />
                    }
                  >
                    <Settings aria-hidden />
                    <span>{t('App.Settings')}</span>
                  </SidebarMenuButton>
                </SidebarMenuItem>
              </SidebarMenu>
            </SidebarGroupContent>
          </SidebarGroup>
          {page === 'chat' ? <ConversationList chat={chat} /> : null}
        </SidebarContent>
        <SidebarFooter>
          <StatusDot
            id="sidebar-agent-state"
            data-state={state}
            tone={tones[state]}
            label={
              <>
                <span>{t('App.AgentState')}</span>{' '}
                <span id="sidebar-agent-state-word">{state}</span>
              </>
            }
          />
        </SidebarFooter>
      </Sidebar>
      <SidebarInset
        render={<main className={page === 'chat' ? styles.contentChat : styles.content} />}
      >
        {needsMove ? (
          <Alert id="needs-move" role="alert">
            {t('App.NeedsMove')}
          </Alert>
        ) : null}
        {page === 'chat' ? (
          <ChatPage
            chat={chat}
            modelSet={modelSet}
            onOpenSettings={() => {
              setPage('settings');
            }}
          />
        ) : (
          <div className={styles.column}>
            {page === 'status' ? <StatusPage status={status} /> : <SettingsPage />}
          </div>
        )}
      </SidebarInset>
    </SidebarProvider>
  );
}
