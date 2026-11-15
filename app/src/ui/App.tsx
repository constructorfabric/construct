import {
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
import { Activity, Settings } from 'lucide-react';
import { useState } from 'react';
import type { CSSProperties, ReactNode } from 'react';
import { t } from '../translation';
import styles from './app.module.css';
import { SettingsPage } from './SettingsPage';
import { tones } from './state-tones';
import { StatusPage } from './StatusPage';
import { useAgentStatus } from './useAgentStatus';

type Page = 'status' | 'settings';

export function App(): ReactNode {
  // A page is a piece of state: the app has no addresses to navigate to.
  const [page, setPage] = useState<Page>('status');
  const status = useAgentStatus();
  const state = status?.state ?? 'unknown';

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
      <SidebarInset render={<main className={styles.content} />}>
        <div className={styles.column}>
          {page === 'status' ? <StatusPage status={status} /> : <SettingsPage />}
        </div>
      </SidebarInset>
    </SidebarProvider>
  );
}
