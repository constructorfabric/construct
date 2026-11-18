import {
  Alert,
  Card,
  CardContent,
  CardDescription,
  CardHeader,
  CardTitle,
  Heading,
  Progress,
  Table,
  TableBody,
  TableCell,
  TableHead,
  TableHeader,
  TableRow,
} from '@gears-frontx/ui-kit';
import { Bar, BarChart, CartesianGrid, XAxis, YAxis } from 'recharts';
import { useEffect, useRef } from 'react';
import type { ReactNode } from 'react';
import type {
  DashboardDocument,
  DayMinutes,
  GroupBy,
  GroupMinutes,
  ProjectChange,
  Whose,
  Widget,
} from '../dashboard-protocol';
import { t } from '../translation';
import styles from './app.module.css';
import { formatMinutes } from './dashboard-period';
import type { Split, WidgetNumbers } from './useDashboard';
import { takeRowsBack } from './focus-after-show';
import { ProjectMenu } from './ProjectMenu';
import type { ProjectActions } from './ProjectMenu';
import { useSize } from './useSize';

/** The colours of the two series, from the kit's chart tokens. */
const seriesColour = { own: 'var(--chart-3)', unattended: 'var(--chart-2)' } as const;

type Series = keyof typeof seriesColour;

/** A day label, `2026-11-16`, as `Mon 16`. */
function shortDay(label: string): string {
  const [year, month, day] = label.split('-').map(Number);
  if (year === undefined || month === undefined || day === undefined) {
    return label;
  }
  return new Date(year, month - 1, day).toLocaleDateString('en-US', {
    weekday: 'short',
    day: 'numeric',
  });
}

function WidgetCard({
  title,
  period,
  children,
}: {
  readonly title: string;
  readonly period: string;
  readonly children: ReactNode;
}): ReactNode {
  return (
    <Card className={styles.widgetCard}>
      <CardHeader>
        <CardTitle render={<h2 />}>{title}</CardTitle>
        <CardDescription>{period}</CardDescription>
      </CardHeader>
      <CardContent className={styles.widgetBody}>{children}</CardContent>
    </Card>
  );
}

function Total({ id, numbers }: { readonly id: string; readonly numbers: number }): ReactNode {
  return (
    <Heading id={id} typography="display" render={<p />}>
      {formatMinutes(numbers)}
    </Heading>
  );
}

/** Time per day as bars, with a table of the same numbers for a reader that cannot see them. */
function Bars({
  id,
  days,
  split,
  sessions,
}: {
  readonly id: string;
  readonly days: readonly DayMinutes[];
  readonly split: readonly Split[] | null;
  readonly sessions: Whose;
}): ReactNode {
  const [ref, { width, height }] = useSize<HTMLDivElement>();
  const single: Series = sessions === 'unattended' ? 'unattended' : 'own';
  const series: Series[] = split === null ? [single] : ['own', 'unattended'];
  const rows = days.map((day, index) => ({
    label: day.label,
    own: split?.[index]?.own ?? (single === 'own' ? day.minutes : 0),
    unattended: split?.[index]?.unattended ?? (single === 'unattended' ? day.minutes : 0),
  }));
  return (
    <figure id={id} className={styles.chart}>
      <ul className={styles.legend}>
        {series.map((name) => (
          <li key={name} className={styles.legendItem}>
            <span className={styles.swatch} style={{ backgroundColor: seriesColour[name] }} />
            {t(`DashboardPage.Series.${name}`)}
          </li>
        ))}
      </ul>
      <div ref={ref} className={styles.chartArea} aria-hidden="true">
        {width > 0 && height > 0 && (
          <BarChart
            width={width}
            height={height}
            data={rows}
            accessibilityLayer={false}
            margin={{ top: 4, right: 4, bottom: 0, left: 0 }}
          >
            <CartesianGrid vertical={false} stroke="var(--border)" />
            <XAxis
              dataKey="label"
              tickFormatter={shortDay}
              tick={{ fill: 'var(--muted-foreground)' }}
              stroke="var(--border)"
              interval="preserveStartEnd"
            />
            <YAxis
              width={96}
              tickFormatter={formatMinutes}
              tick={{ fill: 'var(--muted-foreground)' }}
              stroke="var(--border)"
              allowDecimals={false}
            />
            {series.map((name) => (
              <Bar
                key={name}
                dataKey={name}
                stackId="time"
                fill={seriesColour[name]}
                isAnimationActive={false}
              />
            ))}
          </BarChart>
        )}
      </div>
      <div className="sr-only">
        <table>
          <caption>{t('DashboardPage.Chart.Table')}</caption>
          <thead>
            <tr>
              <th scope="col">{t('DashboardPage.Column.Group.day')}</th>
              {series.map((name) => (
                <th key={name} scope="col">
                  {t(`DashboardPage.Series.${name}`)}
                </th>
              ))}
            </tr>
          </thead>
          <tbody>
            {rows.map((row) => (
              <tr key={row.label}>
                <th scope="row">{row.label}</th>
                {series.map((name) => (
                  <td key={name}>{formatMinutes(row[name])}</td>
                ))}
              </tr>
            ))}
          </tbody>
        </table>
      </div>
    </figure>
  );
}

/** The groups, longest time first. */
function byTime(groups: readonly GroupMinutes[]): GroupMinutes[] {
  return [...groups].sort((a, b) => b.minutes - a.minutes || a.key.localeCompare(b.key));
}

/** The names the document gives a row: the projects shown under it by a name of the page. */
function projectsOf(document: DashboardDocument, name: string): string[] {
  const named = Object.entries(document.projectNames)
    .filter(([, shown]) => shown === name)
    .map(([project]) => project);
  // A project the document has not named is known by the name the row has.
  return Object.hasOwn(document.projectNames, name) ? named : [name, ...named];
}

function GroupTable({
  id,
  groupBy,
  groups,
  actions,
}: {
  readonly id: string;
  readonly groupBy: GroupBy;
  readonly groups: readonly GroupMinutes[];
  /** What a row of projects can do; tables of other groupings have none. */
  readonly actions: ProjectActions | null;
}): ReactNode {
  // Where the focus goes once the rows are counted again: the row with this name, or the first
  // one when there is none. A row hidden or renamed is gone, with the button that had the focus.
  const focusAfter = useRef<string | null | undefined>(undefined);
  useEffect(() => {
    const wanted = focusAfter.current;
    if (wanted === undefined) {
      return;
    }
    focusAfter.current = undefined;
    const buttons = [...window.document.querySelectorAll<HTMLElement>(`#${id} [data-project]`)];
    (buttons.find((button) => button.dataset.project === wanted) ?? buttons[0])?.focus();
  }, [groups, id]);
  // The Show button of the page took the focus away when the rows came back: it goes to the first.
  useEffect(() => {
    if (takeRowsBack() && actions !== null) {
      window.document.querySelector<HTMLElement>(`#${id} [data-project]`)?.focus();
    }
    // Only when the table is drawn for the first time.
  }, []);
  const rowActions: ProjectActions | null =
    actions === null
      ? null
      : {
          hide: (name) => {
            focusAfter.current = null;
            actions.hide(name);
          },
          rename: (name, to) => {
            focusAfter.current = to;
            actions.rename(name, to);
          },
        };
  if (groups.length === 0) {
    return <p className={styles.tableEmpty}>{t('DashboardPage.Table.Empty')}</p>;
  }
  return (
    <Table
      id={id}
      label={t(`DashboardPage.Widget.Table.${groupBy}`)}
      density="compact"
      containerClassName={styles.tableScroll}
    >
      <TableHeader>
        <TableRow>
          <TableHead>{t(`DashboardPage.Column.Group.${groupBy}`)}</TableHead>
          <TableHead>{t('DashboardPage.Column.Time')}</TableHead>
          <TableHead>{t('DashboardPage.Column.Share')}</TableHead>
          <TableHead>{t('DashboardPage.Column.Sessions')}</TableHead>
        </TableRow>
      </TableHeader>
      <TableBody>
        {byTime(groups).map((group) => {
          const percent = Math.round(group.share * 100);
          return (
            <TableRow key={group.key}>
              <TableCell>
                <span className={styles.groupName}>
                  {group.key}
                  {rowActions !== null && <ProjectMenu name={group.key} actions={rowActions} />}
                </span>
              </TableCell>
              <TableCell>{formatMinutes(group.minutes)}</TableCell>
              <TableCell>
                <span className={styles.share}>
                  <Progress
                    className={styles.shareBar}
                    value={percent}
                    aria-label={t('DashboardPage.Share.Of', { name: group.key })}
                  />
                  <span className={styles.shareText}>{percent}%</span>
                </span>
              </TableCell>
              <TableCell>{group.sessions}</TableCell>
            </TableRow>
          );
        })}
      </TableBody>
    </Table>
  );
}

/** One widget of the document with its numbers. */
export function DashboardWidget({
  document,
  widget,
  numbers,
  onChange,
}: {
  readonly document: DashboardDocument;
  readonly widget: Widget;
  readonly numbers: WidgetNumbers | undefined;
  readonly onChange: (changes: readonly ProjectChange[]) => void;
}): ReactNode {
  const period = t(`DashboardPage.Period.${widget.period ?? document.period}`);
  const sessions = widget.sessions ?? document.sessions;
  const groupBy = widget.groupBy ?? 'project';
  const title =
    widget.kind === 'table'
      ? t(`DashboardPage.Widget.Table.${groupBy}`)
      : widget.kind === 'bars_per_day'
        ? t('DashboardPage.Widget.Bars')
        : t('DashboardPage.Widget.Total');
  const actions: ProjectActions | null =
    groupBy === 'project'
      ? {
          hide: (name) => onChange([{ action: 'hide', project: name }]),
          rename: (name, to) =>
            onChange(
              projectsOf(document, name).map((project) => ({
                action: 'rename',
                project,
                name: to,
              })),
            ),
        }
      : null;
  return (
    <WidgetCard title={title} period={period}>
      {numbers === undefined ? null : !numbers.ok ? (
        <Alert variant="destructive">
          {t('DashboardPage.Unavailable', { reason: numbers.error })}
        </Alert>
      ) : widget.kind === 'total' ? (
        <Total id={`value-${widget.id}`} numbers={numbers.numbers.totalMinutes} />
      ) : widget.kind === 'bars_per_day' ? (
        <Bars
          id={`chart-${widget.id}`}
          days={numbers.numbers.days}
          split={numbers.split}
          sessions={sessions}
        />
      ) : (
        <GroupTable
          id={`table-${widget.id}`}
          groupBy={groupBy}
          groups={numbers.numbers.groups}
          actions={actions}
        />
      )}
    </WidgetCard>
  );
}
