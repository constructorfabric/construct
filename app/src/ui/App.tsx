import {
  Alert,
  Sidebar,
  SidebarContent,
  SidebarFooter,
  SidebarGroup,
  SidebarGroupContent,
  SidebarInset,
  SidebarMenu,
  SidebarProvider,
} from '@gears-frontx/ui-kit';
import { Activity, Brain, ChartColumn, MessageSquare, Settings } from 'lucide-react';
import type { LucideIcon } from 'lucide-react';
import { useEffect, useRef, useState } from 'react';
import type { CSSProperties, ReactNode } from 'react';
import { t } from '../translation';
import styles from './app.module.css';
import { ChatPage } from './ChatPage';
import type { ThreadPosition } from './ChatPage';
import { AgentStatusLine } from './components/AgentStatusLine';
import { ConnectorIndicators } from './components/ConnectorIndicators';
import { MenuItem } from './components/MenuItem';
import { ConversationList } from './ConversationList';
import { DashboardPage } from './DashboardPage';
import { MemoryPage } from './MemoryPage';
import { SettingsPage } from './SettingsPage';
import { StatusPage } from './StatusPage';
import { useAgentStatus } from './useAgentStatus';
import { useChat } from './useChat';
import { useErasing } from './useErasing';

type Page = 'chat' | 'dashboard' | 'memory' | 'status' | 'settings';

const pages = [
  { page: 'chat', icon: MessageSquare, label: 'App.Chat' },
  { page: 'dashboard', icon: ChartColumn, label: 'App.Dashboard' },
  { page: 'memory', icon: Brain, label: 'App.Memory' },
  { page: 'status', icon: Activity, label: 'App.Status' },
  { page: 'settings', icon: Settings, label: 'App.Settings' },
] as const satisfies readonly { page: Page; icon: LucideIcon; label: string }[];

export function App(): ReactNode {
  // A page is a piece of state: the app has no addresses to navigate to.
  const [page, setPage] = useState<Page>('chat');
  const { status, refresh } = useAgentStatus();
  const state = status?.state ?? 'unknown';
  const chat = useChat();
  // Where the thread was scrolled to, kept here because the chat page is gone on other pages.
  const threadPosition = useRef<ThreadPosition>({ atBottom: true, top: 0 });
  const erasing = useErasing(chat.reload);
  // The model may have been set in Settings a moment ago; the chat page must not say otherwise.
  useEffect(() => {
    if (page === 'chat') {
      refresh();
    }
  }, [page, refresh]);
  // Only an agent that says so has no model; one that cannot be reached is not "without a model".
  const modelSet = status?.state !== 'running' || status.model !== 'not set';
  // Only an agent that tells the gear's state and says it is not ready has no profile to give.
  const profileAvailable =
    status?.state !== 'running' ||
    status.construct === undefined ||
    status.construct.state === 'ready';
  // Kept here: a poll that fails drops the group, and it must come back as the person left it.
  const [collectionOpen, setCollectionOpen] = useState(true);
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
                {pages.map(({ page: target, icon: Icon, label }) => (
                  <MenuItem
                    key={target}
                    icon={<Icon aria-hidden />}
                    label={t(label)}
                    active={page === target}
                    onSelect={() => setPage(target)}
                  />
                ))}
              </SidebarMenu>
            </SidebarGroupContent>
          </SidebarGroup>
          {page === 'chat' ? <ConversationList chat={chat} /> : null}
        </SidebarContent>
        <SidebarFooter>
          <AgentStatusLine state={state} />
          {status?.state === 'running' && status.connectors !== undefined ? (
            <ConnectorIndicators
              connectors={status.connectors}
              open={collectionOpen}
              onOpenChange={setCollectionOpen}
            />
          ) : null}
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
            position={threadPosition.current}
            modelSet={modelSet}
            profileAvailable={profileAvailable}
            onOpenSettings={() => {
              setPage('settings');
            }}
          />
        ) : (
          <div className={styles.column}>
            {page === 'status' ? (
              <StatusPage status={status} />
            ) : page === 'dashboard' ? (
              <DashboardPage status={status} onOpenSettings={() => setPage('settings')} />
            ) : page === 'memory' ? (
              <MemoryPage onOpenSettings={() => setPage('settings')} />
            ) : (
              <SettingsPage
                chat={chat}
                erasing={erasing}
                status={status}
                onOpenDashboard={() => setPage('dashboard')}
              />
            )}
          </div>
        )}
      </SidebarInset>
    </SidebarProvider>
  );
}
