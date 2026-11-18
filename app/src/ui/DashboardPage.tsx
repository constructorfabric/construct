import {
  Alert,
  Button,
  Card,
  CardContent,
  CardHeader,
  CardTitle,
  DatePicker,
  Empty,
  EmptyActions,
  EmptyDescription,
  EmptyHeader,
  EmptyTitle,
  Heading,
  Spinner,
  Text,
  ToggleGroup,
  ToggleGroupItem,
} from '@gears-frontx/ui-kit';
import { addDays, startOfDay } from 'date-fns';
import { ChevronLeft, ChevronRight, ChevronsLeft, ChevronsRight } from 'lucide-react';
import { useState } from 'react';
import type { ReactNode } from 'react';
import { isWhose, whoseValues } from '../dashboard-protocol';
import type { Whose } from '../dashboard-protocol';
import { t } from '../translation';
import styles from './app.module.css';
import { formatMinutes, mondayOf, shortDate } from './dashboard-period';
import { useDashboard } from './useDashboard';
import type { DashboardState } from './useDashboard';

function Total({
  id,
  title,
  value,
}: {
  readonly id: string;
  readonly title: string;
  readonly value: string;
}): ReactNode {
  return (
    <Card className={styles.card}>
      <CardHeader>
        <CardTitle render={<h2 />}>{title}</CardTitle>
      </CardHeader>
      <CardContent>
        <Text id={id} typography="kpi" render={<p />}>
          {value}
        </Text>
      </CardContent>
    </Card>
  );
}

function Body({
  state,
  day,
  onOpenSettings,
}: {
  readonly state: DashboardState;
  readonly day: Date;
  readonly onOpenSettings: () => void;
}): ReactNode {
  if (state.kind === 'reading') {
    return <Spinner id="dashboard-reading" label={t('DashboardPage.Reading')} />;
  }
  if (state.kind === 'failed') {
    return (
      <Alert id="dashboard-failed" variant="destructive">
        {t('DashboardPage.Unavailable', { reason: state.reason })}
      </Alert>
    );
  }
  const { collecting } = state;
  if (state.sessionCount === 0 && state.totalMinutes === 0) {
    return collecting ? (
      <Empty id="dashboard-empty">
        <EmptyHeader>
          <EmptyTitle render={<h2 />}>{t('DashboardPage.Empty.Title')}</EmptyTitle>
          <EmptyDescription>{t('DashboardPage.Empty.Text')}</EmptyDescription>
        </EmptyHeader>
      </Empty>
    ) : (
      <Empty id="dashboard-off">
        <EmptyHeader>
          <EmptyTitle render={<h2 />}>{t('DashboardPage.Off.Title')}</EmptyTitle>
          <EmptyDescription>{t('DashboardPage.Off.Text')}</EmptyDescription>
        </EmptyHeader>
        <EmptyActions>
          <Button id="dashboard-open-settings" variant="link" onClick={onOpenSettings}>
            {t('DashboardPage.OpenSettings')}
          </Button>
        </EmptyActions>
      </Empty>
    );
  }
  return (
    <>
      {!collecting && (
        <Alert id="dashboard-off-note">
          <span className={styles.alertContent}>
            {t('DashboardPage.Off.Note')}
            <Button
              id="dashboard-open-settings"
              variant="outline"
              size="sm"
              onClick={onOpenSettings}
            >
              {t('DashboardPage.OpenSettings')}
            </Button>
          </span>
        </Alert>
      )}
      <Total
        id="dashboard-week-total"
        title={t('DashboardPage.Week')}
        value={formatMinutes(state.totalMinutes)}
      />
      <Total
        id="dashboard-day-total"
        title={t('DashboardPage.Day', { day: shortDate(day) })}
        value={formatMinutes(state.dayMinutes)}
      />
      <Total
        id="dashboard-sessions"
        title={t('DashboardPage.Sessions')}
        value={String(state.sessionCount)}
      />
    </>
  );
}

/** The dashboard: the numbers of one week, for the user's own time, the unattended or both. */
export function DashboardPage({
  onOpenSettings,
}: {
  readonly onOpenSettings: () => void;
}): ReactNode {
  const [day, setDay] = useState(() => startOfDay(new Date()));
  const [whose, setWhose] = useState<Whose>('own');
  const state = useDashboard(day, whose);
  const first = mondayOf(day);
  const move = (days: number): void => {
    setDay((current) => addDays(current, days));
  };

  return (
    <>
      <Heading typography="display" render={<h1 />}>
        {t('DashboardPage.Title')}
      </Heading>
      <div className={styles.toolbar}>
        <div
          className={styles.toolbarGroup}
          role="group"
          aria-label={t('DashboardPage.Navigation')}
        >
          <Button
            id="dashboard-previous-week"
            variant="outline"
            size="sm"
            aria-label={t('DashboardPage.Previous.Week')}
            icon={<ChevronsLeft />}
            onClick={() => move(-7)}
          />
          <Button
            id="dashboard-previous-day"
            variant="outline"
            size="sm"
            aria-label={t('DashboardPage.Previous.Day')}
            icon={<ChevronLeft />}
            onClick={() => move(-1)}
          />
          <DatePicker
            id="dashboard-date"
            selected={day}
            onSelect={(picked) => {
              if (picked !== undefined) {
                setDay(startOfDay(picked));
              }
            }}
            placeholder={t('DashboardPage.PickDate')}
          />
          <Button
            id="dashboard-next-day"
            variant="outline"
            size="sm"
            aria-label={t('DashboardPage.Next.Day')}
            icon={<ChevronRight />}
            onClick={() => move(1)}
          />
          <Button
            id="dashboard-next-week"
            variant="outline"
            size="sm"
            aria-label={t('DashboardPage.Next.Week')}
            icon={<ChevronsRight />}
            onClick={() => move(7)}
          />
        </div>
        <ToggleGroup
          id="dashboard-whose"
          variant="outline"
          size="sm"
          aria-label={t('DashboardPage.Whose')}
          value={[whose]}
          onValueChange={([next]) => {
            // Pressing the chosen item again would leave none: the choice stays.
            if (isWhose(next)) {
              setWhose(next);
            }
          }}
        >
          {whoseValues.map((value) => (
            <ToggleGroupItem key={value} value={value} id={`dashboard-whose-${value}`}>
              {t(`DashboardPage.Whose.${value}`)}
            </ToggleGroupItem>
          ))}
        </ToggleGroup>
      </div>
      <Text id="dashboard-period" tone="muted" className={styles.period}>
        {t('DashboardPage.Period', { first: shortDate(first), last: shortDate(addDays(first, 6)) })}
      </Text>
      <Body state={state} day={day} onOpenSettings={onOpenSettings} />
    </>
  );
}
