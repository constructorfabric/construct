import { describe, expect, it } from 'vitest';
import type { Widget } from '../src/dashboard-protocol';
import { drillSource } from '../src/ui/drill-source';

const day = { label: '2026-11-16', start: '2026-11-16T00:00:00Z', end: '2026-11-17T00:00:00Z' };
const position = { column: 0, row: 0, width: 6, height: 4 };

describe('drillSource', () => {
  it('asks for a row of a table by the table widget itself', () => {
    const table: Widget = { id: 'by-project', kind: 'table', groupBy: 'project', position };
    expect(drillSource(table, [day], 'own', 'billing')).toEqual({
      days: [day],
      group: 'billing',
      widget: 'by-project',
    });
  });

  it('asks for a bar of a chart as a day, with the sessions and the filter of the chart', () => {
    const chart: Widget = {
      id: 'per-day',
      kind: 'bars_per_day',
      filter: { tools: ['codex'] },
      position,
    };
    expect(drillSource(chart, [day], 'all', '2026-11-16')).toEqual({
      days: [day],
      group: '2026-11-16',
      sessions: 'all',
      groupBy: 'day',
      filter: { tools: ['codex'] },
    });
  });

  it('leaves the filter out when the chart has none', () => {
    const chart: Widget = { id: 'per-day', kind: 'bars_per_day', position };
    expect(drillSource(chart, [day], 'own', '2026-11-16')).not.toHaveProperty('filter');
  });
});
