# The dashboard

The dashboard page is drawn from one JSON file, `dashboards/activity.json`, in the workspace;
`dashboard.schema.json` next to this guide is its schema. Edit the file with `Read` and `Edit`,
then call `render_dashboard`. The page changes when the file is valid; when it is not, the page
keeps the last good version and `render_dashboard` lists the errors with their places.

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
      { "id": "by-tool", "kind": "table", "group_by": "tool", "period": "last_7_days", "sessions": "all", "position": { "column": 0, "row": 2, "width": 12, "height": 5 } }
    ],
    "hidden_projects": [],
    "project_names": {}
  }
}
```

Keep `type` and `title` as they are unless asked. `payload.period`, `sessions` and `source` are the
defaults of every widget; a widget may have its own.

## What it can show

- `kind`: `total` (one number), `bars_per_day` (a bar for each day), `table` (time by group, needs
  `group_by`) and `day_summary` (the summary of the end of the day).
- `source`: `ai_sessions` (the time of AI sessions; what no `source` reads as) or `on_screen` (the
  time each application and site was in front on this computer).
- `group_by`, for a table only. `ai_sessions`: `project`, `ticket`, `tool`, `collection` or `day`.
  `on_screen`: `project`, `ticket`, `app`, `site` or `day`. Time with none is `No site`,
  `No project` or `No ticket`.
- `period`: `today`, `this_week`, `last_week`, `this_month`, `last_month`, `last_7_days`, `last_30_days`.
- `sessions` (`ai_sessions` only): `own` (the user's prompts), `unattended` (scripts) or `all`.
- `filter`: `projects`, `tickets` and `tools` (`ai_sessions` only), each a list of names.
- `payload.hidden_projects` (projects left out) and `payload.project_names` (a name to show for a
  project, by its own name) apply to both sources.

The dashboard shows the time of AI sessions and the time on screen only: no window title, no
address, no scripts or formulas. `day_summary` is of `ai_sessions` only.

## The grid

The grid has 12 columns. A `position` has `column` (0 to 11), `row` (from 0, downwards), `width`
(1 to 12) and `height` (1 or more, in rows). A widget must fit: `column` + `width` is at most 12.
Widgets do not move each other: give each its own place, with no overlap. Each `id` is unique and
uses only lower-case letters, digits, `-` and `_`. To move a widget, change its `column` and `row`;
to resize it, its `width` and `height`. To add one, add an object to `widgets` with a new `id`; to
remove one, delete its object. For example, this puts a wider `by-tool` above a taller
`by-project`, and adds the time on screen:

```json
{
  "type": "gts.cf.construct.core.dashboard.v1~cf.construct.dashboard.activity.v1~",
  "title": "Activity",
  "payload": {
    "period": "this_week",
    "sessions": "own",
    "widgets": [
      { "id": "by-tool", "kind": "table", "group_by": "tool", "position": { "column": 0, "row": 0, "width": 12, "height": 4 } },
      { "id": "by-project", "kind": "table", "group_by": "project", "filter": { "projects": ["shop"] }, "position": { "column": 0, "row": 4, "width": 12, "height": 6 } },
      { "id": "by-app", "kind": "table", "source": "on_screen", "group_by": "app", "position": { "column": 0, "row": 10, "width": 12, "height": 5 } }
    ],
    "hidden_projects": ["scratch"],
    "project_names": { "shop": "Online shop" }
  }
}
```

## After an edit

Call `render_dashboard` with no arguments: it shows every widget with its numbers, or the errors of
the file. Fix the errors and call it again before telling the user the change is made. With
`source`, `period` and `group_by` it answers a question the page does not show.
