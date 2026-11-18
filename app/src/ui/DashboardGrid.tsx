import GridLayout from 'react-grid-layout';
import type { Layout } from 'react-grid-layout';
import { useState } from 'react';
import type { ReactNode } from 'react';
import type { Widget } from '../dashboard-protocol';
import styles from './app.module.css';
import { useSize } from './useSize';

/** The columns of the grid, as the document counts them. */
const columns = 12;
/** The height of a row, and the gap between widgets, in pixels. */
const rowHeight = 56;
const gap = 16;

function layoutOf(widgets: readonly Widget[]): Layout[] {
  return widgets.map(({ id, position }) => ({
    i: id,
    x: position.column,
    y: position.row,
    w: position.width,
    h: position.height,
  }));
}

/**
 * The widgets on a grid, each at the column, row, width and height of the document. The width of
 * the grid is the width of the space it is in. `arrangeable` lets the user drag a widget and
 * resize it with the mouse; the new places are kept here, not written to the document.
 */
export function DashboardGrid({
  widgets,
  arrangeable,
  render,
}: {
  readonly widgets: readonly Widget[];
  readonly arrangeable: boolean;
  readonly render: (widget: Widget) => ReactNode;
}): ReactNode {
  const [ref, { width }] = useSize<HTMLDivElement>();
  // A new document starts from its own places: the ones the user made belong to the old one.
  const [kept, setKept] = useState<{ widgets: readonly Widget[]; layout: Layout[] }>({
    widgets,
    layout: layoutOf(widgets),
  });
  let layout = kept.layout;
  if (kept.widgets !== widgets) {
    layout = layoutOf(widgets);
    setKept({ widgets, layout });
  }
  const places = new Map(layout.map((item) => [item.i, item]));

  return (
    <div ref={ref} className={styles.grid}>
      {width > 0 && (
        <GridLayout
          width={width}
          cols={columns}
          rowHeight={rowHeight}
          margin={[gap, gap]}
          containerPadding={[0, 0]}
          layout={layout}
          // The document says where a widget is: nothing slides up to fill a gap.
          compactType={null}
          isDraggable={arrangeable}
          isResizable={arrangeable}
          onLayoutChange={(next) => {
            setKept((current) => ({ ...current, layout: next }));
          }}
        >
          {widgets.map((widget) => {
            const place = places.get(widget.id);
            return (
              <div
                key={widget.id}
                id={`widget-${widget.id}`}
                className={styles.widget}
                data-column={place?.x}
                data-row={place?.y}
                data-width={place?.w}
                data-height={place?.h}
              >
                {render(widget)}
              </div>
            );
          })}
        </GridLayout>
      )}
    </div>
  );
}
