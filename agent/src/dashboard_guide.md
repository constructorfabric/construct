# The dashboard

The dashboard page is drawn from one JSON file, `dashboards/activity.json`, in the workspace.
`dashboard.schema.json` next to this guide is the schema of that file. Edit the file with `Read`
and `Edit`, then call `render_dashboard` to see the result. The page changes by itself when the
file is valid. When it is not, the page keeps the last good version and `render_dashboard` lists
the errors with their places.

## The file

```json
{
  "type": "gts.cf.construct.core.dashboard.v1~cf.construct.dashboard.activity.v1~",
  "title": "Activity",
  "payload": {
    "period": "this_week",
    "sessions": "own",
    "widgets": [
      { "id": "total", "kind": "total", "position": { "column": 0, "row": 0, "width": 12, "height": 2 } },
      { "id": "by-project", "kind": "table", "group_by": "project", "position": { "column": 0, "row": 2, "width": 6, "height": 5 } },
      { "id": "by-tool", "kind": "table", "group_by": "tool", "period": "last_7_days", "sessions": "all", "position": { "column": 6, "row": 2, "width": 6, "height": 5 } }
    ],
    "hidden_projects": [],
    "project_names": {}
  }
}
```

Keep `type` and `title` as they are unless asked. `payload.period` and `payload.sessions` are the
defaults of every widget; a widget may have its own.

## What it can show

- `kind`: `total` (one number, the time of the period), `bars_per_day` (a bar for each day),
  `table` (time by group, needs `group_by`) and `day_summary` (the summary of the end of the day).
- `group_by`, for a table only: `project`, `ticket`, `tool` or `day`.
- `period`: `today`, `this_week`, `last_week`, `this_month`, `last_month`, `last_7_days` or
  `last_30_days`.
- `sessions`: `own` (the user's prompts), `unattended` (scripts) or `all`.
- `filter`: `projects`, `tickets` and `tools`, each a list of names; a list left out means no limit.
- `payload.hidden_projects`: projects the page leaves out. `payload.project_names`: the name to show
  for a project, by its own name.

The dashboard shows time spent in sessions and nothing else. It has no scripts or formulas.

## The grid

The grid has 12 columns. A `position` has `column` (0 to 11), `row` (from 0, downwards), `width`
(1 to 12) and `height` (1 or more, in rows). A widget must fit: `column` + `width` is at most 12.
Widgets do not move each other, so give each its own place and keep them from overlapping. Each
`id` is unique and uses only lower-case letters, digits, `-` and `_`.

To move a widget, change its `column` and `row`. To resize it, change its `width` and `height`.
To add one, add an object to `widgets` with a new `id`. To remove one, delete its object.

For example, this moves `by-project` below a wider `by-tool` and makes it taller:

```json
{
  "type": "gts.cf.construct.core.dashboard.v1~cf.construct.dashboard.activity.v1~",
  "title": "Activity",
  "payload": {
    "period": "this_week",
    "sessions": "own",
    "widgets": [
      { "id": "by-tool", "kind": "table", "group_by": "tool", "position": { "column": 0, "row": 0, "width": 12, "height": 4 } },
      { "id": "by-project", "kind": "table", "group_by": "project", "filter": { "projects": ["shop"] }, "position": { "column": 0, "row": 4, "width": 12, "height": 6 } }
    ],
    "hidden_projects": ["scratch"],
    "project_names": { "shop": "Online shop" }
  }
}
```

## After an edit

Call `render_dashboard` with no arguments. It shows every widget with its numbers, or the errors of
the file. Fix the errors and call it again before telling the user the change is made.
