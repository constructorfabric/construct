import {
  Alert,
  Button,
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
  Spinner,
  Table,
  TableBody,
  TableCell,
  TableHead,
  TableHeader,
  TableRow,
} from '@gears-frontx/ui-kit';
import { format } from 'date-fns';
import type { ReactNode } from 'react';
import { t } from '../translation';
import styles from './app.module.css';
import { formatMinutes, secondsText } from './dashboard-period';
import { useSessions } from './useSessions';
import type { SessionsSource } from './useSessions';

/** A moment the agent gives as a UTC instant, as `2026-11-17 09:05` on the user's own clock. */
export function clockText(time: string): string {
  const at = new Date(time);
  return format(at, 'yyyy-MM-dd HH:mm');
}

/**
 * The sessions behind one row of a table or one bar of a chart, a page at a time; for the time on
 * screen (`screen`), the blocks of time behind it, which hold no title and no address.
 */
export function SessionsDialog({
  source,
  screen,
  onClose,
}: {
  readonly source: SessionsSource;
  readonly screen: boolean;
  readonly onClose: () => void;
}): ReactNode {
  const list = useSessions(source);
  return (
    <Dialog
      open
      onOpenChange={(open) => {
        if (!open) {
          onClose();
        }
      }}
    >
      <DialogContent
        className={styles.drillContent}
        closeLabel={t(screen ? 'DashboardPage.Drill.Blocks.Close' : 'DashboardPage.Drill.Close')}
      >
        <DialogHeader>
          <DialogTitle>
            {t(screen ? 'DashboardPage.Drill.Blocks.Title' : 'DashboardPage.Drill.Title', {
              group: source.group,
            })}
          </DialogTitle>
          <DialogDescription id="drill-count">
            {list.total === null
              ? t(screen ? 'DashboardPage.Drill.Blocks.Loading' : 'DashboardPage.Drill.Loading')
              : t(screen ? 'DashboardPage.Drill.Blocks.Count' : 'DashboardPage.Drill.Count', {
                  shown: list.list?.rows.length ?? 0,
                  total: list.total,
                })}
          </DialogDescription>
        </DialogHeader>
        {list.list?.kind === 'blocks' && list.list.rows.length > 0 && (
          <Table
            id="drill-blocks"
            label={t('DashboardPage.Drill.Blocks.Table')}
            density="compact"
            containerClassName={styles.drillScroll}
          >
            <TableHeader>
              <TableRow>
                <TableHead>{t('DashboardPage.Drill.Start')}</TableHead>
                <TableHead>{t('DashboardPage.Drill.End')}</TableHead>
                <TableHead>{t('DashboardPage.Column.Time')}</TableHead>
                <TableHead>{t('DashboardPage.Column.Group.app')}</TableHead>
                <TableHead>{t('DashboardPage.Column.Group.site')}</TableHead>
                <TableHead>{t('DashboardPage.Column.Group.project')}</TableHead>
                <TableHead>{t('DashboardPage.Column.Group.ticket')}</TableHead>
              </TableRow>
            </TableHeader>
            <TableBody>
              {list.list.rows.map((row, index) => (
                <TableRow key={`${row.start}-${index}`}>
                  <TableCell>{clockText(row.start)}</TableCell>
                  <TableCell>{clockText(row.end)}</TableCell>
                  <TableCell>{secondsText(row.seconds)}</TableCell>
                  <TableCell>{row.app}</TableCell>
                  <TableCell>{row.site}</TableCell>
                  <TableCell>{row.project}</TableCell>
                  <TableCell>{row.ticket}</TableCell>
                </TableRow>
              ))}
            </TableBody>
          </Table>
        )}
        {list.list?.kind === 'sessions' && list.list.rows.length > 0 && (
          <Table
            id="drill-sessions"
            label={t('DashboardPage.Drill.Table')}
            density="compact"
            containerClassName={styles.drillScroll}
          >
            <TableHeader>
              <TableRow>
                <TableHead>{t('DashboardPage.Drill.Start')}</TableHead>
                <TableHead>{t('DashboardPage.Drill.End')}</TableHead>
                <TableHead>{t('DashboardPage.Column.Time')}</TableHead>
                <TableHead>{t('DashboardPage.Column.Group.tool')}</TableHead>
                <TableHead>{t('DashboardPage.Column.Group.project')}</TableHead>
                <TableHead>{t('DashboardPage.Column.Group.ticket')}</TableHead>
                <TableHead>{t('DashboardPage.Drill.Session')}</TableHead>
              </TableRow>
            </TableHeader>
            <TableBody>
              {list.list.rows.map((row, index) => (
                <TableRow key={`${row.session}-${row.start}-${index}`}>
                  <TableCell>{clockText(row.start)}</TableCell>
                  <TableCell>{clockText(row.end)}</TableCell>
                  <TableCell>{formatMinutes(row.minutes)}</TableCell>
                  <TableCell>{row.tool}</TableCell>
                  <TableCell>{row.project}</TableCell>
                  <TableCell>{row.ticket}</TableCell>
                  <TableCell>{row.session}</TableCell>
                </TableRow>
              ))}
            </TableBody>
          </Table>
        )}
        {list.error !== null && (
          <Alert id="drill-failed" variant="destructive">
            {t('DashboardPage.Unavailable', { reason: list.error })}
          </Alert>
        )}
        {list.loading && (
          <Spinner
            id="drill-loading"
            label={t(screen ? 'DashboardPage.Drill.Blocks.Loading' : 'DashboardPage.Drill.Loading')}
          />
        )}
        {!list.loading && (list.hasMore || list.error !== null) && (
          <DialogFooter>
            <Button id="drill-more" variant="outline" onClick={list.more}>
              {list.error === null ? t('DashboardPage.Drill.More') : t('DashboardPage.Drill.Retry')}
            </Button>
          </DialogFooter>
        )}
      </DialogContent>
    </Dialog>
  );
}
