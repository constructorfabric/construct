import { describe, expect, it } from 'vitest';
import type { DashboardDocument, Widget } from '../src/dashboard-protocol';
import { missingGrants, sourceOf, sourcesUsed, gridWidgets } from '../src/ui/dashboard-source';
import { secondsText } from '../src/ui/dashboard-period';

const at = (row: number): Widget['position'] => ({ column: 0, row, width: 12, height: 2 });
const widget = (id: string, row: number, more: Partial<Widget> = {}): Widget => ({
  id,
  kind: 'total',
  position: at(row),
  ...more,
});
const document = (widgets: Widget[], source: DashboardDocument['source']): DashboardDocument => ({
  title: 'Activity',
  period: 'this_week',
  sessions: 'own',
  source,
  widgets,
  hiddenProjects: [],
  projectNames: {},
});

describe('the source of a widget', () => {
  it('is its own, else the page', () => {
    const page = document([widget('a', 0), widget('b', 2, { source: 'on_screen' })], 'ai_sessions');
    expect(page.widgets.map((one) => sourceOf(page, one))).toEqual(['ai_sessions', 'on_screen']);
    const screen = document(
      [widget('a', 0), widget('b', 2, { source: 'ai_sessions' })],
      'on_screen',
    );
    expect(screen.widgets.map((one) => sourceOf(screen, one))).toEqual([
      'on_screen',
      'ai_sessions',
    ]);
  });

  it('gives the sources a page has widgets of, the AI sessions first', () => {
    expect(
      sourcesUsed(
        document([widget('a', 0, { source: 'on_screen' }), widget('b', 2)], 'ai_sessions'),
      ),
    ).toEqual(['ai_sessions', 'on_screen']);
    expect(sourcesUsed(document([widget('a', 0)], 'on_screen'))).toEqual(['on_screen']);
  });
});

describe('the widgets of one source', () => {
  it('moves them up so the first one is at the top, and keeps the rest of the places', () => {
    const page = document(
      [
        widget('ai', 0),
        widget('s1', 14, { source: 'on_screen' }),
        widget('s2', 16, { source: 'on_screen' }),
      ],
      'ai_sessions',
    );
    expect(gridWidgets(page, 'on_screen').map((one) => [one.id, one.position.row])).toEqual([
      ['s1', 0],
      ['s2', 2],
    ]);
    expect(gridWidgets(page, 'ai_sessions').map((one) => one.position.row)).toEqual([0]);
  });

  it('leaves a page with one source where its document puts the widgets', () => {
    const page = document([widget('a', 3), widget('b', 6)], 'ai_sessions');
    expect(gridWidgets(page, 'ai_sessions').map((one) => one.position.row)).toEqual([3, 6]);
    const screen = document([widget('a', 5, { source: 'on_screen' })], 'ai_sessions');
    expect(gridWidgets(screen, 'on_screen').map((one) => one.position.row)).toEqual([5]);
  });
});

describe('the permissions that are missing', () => {
  it('names the ones the person has not given, and not a browser that is closed', () => {
    expect(
      missingGrants([
        { name: 'accessibility', state: 'granted' },
        { name: 'screen_recording', state: 'denied' },
        { name: 'automation', state: 'browser_not_running' },
        { name: 'input_monitoring', state: 'not_asked' },
      ]),
    ).toEqual(['screen_recording', 'input_monitoring']);
    expect(missingGrants(undefined)).toEqual([]);
  });
});

describe('a length of time on screen', () => {
  it('is written in hours, minutes and seconds, from the first part that is not zero', () => {
    expect(secondsText(0)).toBe('0 s');
    expect(secondsText(45)).toBe('45 s');
    expect(secondsText(320)).toBe('5 min 20 s');
    expect(secondsText(3600)).toBe('1 h 0 min 0 s');
    expect(secondsText(3725)).toBe('1 h 2 min 5 s');
  });
});
