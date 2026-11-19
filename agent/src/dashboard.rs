//! The dashboard: a document, kept in the workspace, that is an instance of a registered type.
//!
//! `DashboardV1` is the base type and `ActivityDashboardV1` the first type derived from it; a kind
//! for another audience is one more derived struct. `#[gts_type_schema]` submits each schema to
//! the link-time inventory, which the host's types registry reads when it starts.
//!
//! The document is plain data: no scripts, no expressions. A schema cannot say that a widget
//! stays inside the grid, that ids are unique or that a table names what it groups by, so
//! [`validate`] checks those after the schema.

use gts::{GtsSchema, GtsTypeId};
use jsonschema::{Draft, Validator};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use toolkit_gts::{gts_id, gts_type_schema};

use crate::workspace::{Error, Workspace};

/// The base type every dashboard derives from.
pub const BASE_TYPE: &str = gts_id!("cf.construct.core.dashboard.v1~");
/// The activity dashboard: a developer's time by project and ticket.
pub const ACTIVITY_TYPE: &str =
    gts_id!("cf.construct.core.dashboard.v1~cf.construct.dashboard.activity.v1~");
/// Where the activity dashboard is kept, relative to the workspace.
pub const ACTIVITY_FILE: &str = "dashboards/activity.json";
/// The number of columns of the grid.
pub const GRID_COLUMNS: u32 = 12;

/// The dashboard envelope. `P` is the payload a derived type defines.
#[derive(Debug, Clone, PartialEq)]
#[gts_type_schema(
    dir_path = "schemas",
    base = true,
    gts_abstract = true,
    type_id = gts_id!("cf.construct.core.dashboard.v1~"),
    description = "Abstract base of a dashboard document: the type it is an instance of, a title and the payload a derived type defines. The top level is frozen: a derived type refines only the payload.",
    properties = "gts_type,title,payload"
)]
pub struct DashboardV1<P: GtsSchema> {
    /// The derived type this document is an instance of.
    #[serde(rename = "type")]
    #[schemars(extend(
        "x-gts-type-ref" = gts_id!("cf.construct.core.dashboard.v1~"),
        "maxLength" = 1024,
        "pattern" = r"^gts\.[a-z_][a-z0-9_]*(\.[a-z_][a-z0-9_]*){3}\.v[0-9]+(\.[0-9]+)?~([a-z_][a-z0-9_]*(\.[a-z_][a-z0-9_]*){3}\.v[0-9]+(\.[0-9]+)?~)+$"
    ))]
    pub gts_type: GtsTypeId,
    /// The title the page shows.
    #[schemars(length(min = 1, max = 200))]
    pub title: String,
    /// The content a derived type defines.
    pub payload: P,
}

/// The period a widget covers. Weeks start on Monday.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
#[schemars(inline)]
pub enum Period {
    Today,
    ThisWeek,
    LastWeek,
    ThisMonth,
    LastMonth,
    #[serde(rename = "last_7_days")]
    Last7Days,
    #[serde(rename = "last_30_days")]
    Last30Days,
}

/// Whose sessions a widget counts.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
#[schemars(inline)]
pub enum Sessions {
    /// Sessions the user wrote the prompts of.
    Own,
    /// Sessions of scripts and unattended runs.
    Unattended,
    /// Both.
    All,
}

/// What a widget counts the time of.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
#[schemars(inline)]
pub enum Source {
    /// The time of the sessions of the AI tools. What a document with no source reads as.
    #[default]
    AiSessions,
    /// The time each application and site was in front on this computer.
    OnScreen,
}

/// What a widget draws.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
#[schemars(inline)]
pub enum WidgetKind {
    /// One number: the time of the period.
    Total,
    /// The time of each day of the period as a bar.
    BarsPerDay,
    /// A table of time, grouped by what `group_by` names.
    Table,
    /// The summary of the end of the day.
    DaySummary,
}

/// What a table groups the time by.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
#[schemars(inline)]
pub enum GroupBy {
    Project,
    Ticket,
    /// AI sessions only: the tool that wrote the session.
    Tool,
    /// AI sessions only: the grouping the setup found for the user's work: clients, courses,
    /// topics.
    Collection,
    Day,
    /// On screen only: the application in front.
    App,
    /// On screen only: the site in front, the host of the address of the tab.
    Site,
}

impl GroupBy {
    /// Whether the time of `source` can be grouped this way.
    #[must_use]
    pub const fn fits(self, source: Source) -> bool {
        match self {
            Self::Project | Self::Ticket | Self::Day => true,
            Self::Tool | Self::Collection => matches!(source, Source::AiSessions),
            Self::App | Self::Site => matches!(source, Source::OnScreen),
        }
    }
}

/// What a widget keeps: a name left out means no limit on it.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
#[schemars(inline)]
pub struct Filter {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub projects: Option<Vec<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tickets: Option<Vec<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tools: Option<Vec<String>>,
}

/// Where a widget sits on the grid. Columns and rows count from 0.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
#[schemars(inline)]
pub struct Position {
    #[schemars(range(min = 0, max = 11))]
    pub column: u32,
    #[schemars(range(min = 0, max = 10000))]
    pub row: u32,
    #[schemars(range(min = 1, max = 12))]
    pub width: u32,
    #[schemars(range(min = 1, max = 100))]
    pub height: u32,
}

/// One widget of the page.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
#[schemars(inline)]
pub struct Widget {
    /// Names the widget in the document; unique in it.
    #[schemars(length(min = 1, max = 64), regex(pattern = r"^[a-z0-9_-]+$"))]
    pub id: String,
    pub kind: WidgetKind,
    /// What a table groups by; a table has it and no other widget does.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub group_by: Option<GroupBy>,
    /// The widget's own period; the dashboard's when left out.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub period: Option<Period>,
    /// The widget's own sessions; the dashboard's when left out. Only AI sessions have it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sessions: Option<Sessions>,
    /// What the widget counts the time of; the dashboard's when left out.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source: Option<Source>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub filter: Option<Filter>,
    pub position: Position,
}

/// The payload of the activity dashboard.
#[derive(Debug, Clone, PartialEq, Eq)]
#[gts_type_schema(
    dir_path = "schemas",
    base = DashboardV1,
    type_id = gts_id!("cf.construct.core.dashboard.v1~cf.construct.dashboard.activity.v1~"),
    description = "The activity dashboard: a developer's time by project and ticket. It lists the widgets of the page, each with a kind, a place on a grid of 12 columns and optionally its own period, sessions, source and filter, and holds the projects the page hides and the names it shows for projects. The source of time is AI sessions or On screen, and a document with none reads as AI sessions.",
    properties = "period,sessions,source,widgets,hidden_projects,project_names"
)]
pub struct ActivityDashboardV1 {
    /// The period of every widget that has none of its own.
    pub period: Period,
    /// The sessions of every widget that has none of its own.
    pub sessions: Sessions,
    /// The source of every widget that has none of its own; AI sessions when left out.
    // The type reads every member as present, so a reader of a document that leaves this one out
    // puts a `null` there first (see `payload_of` in the numbers).
    pub source: Option<Source>,
    /// The widgets, in no order: the grid places them.
    pub widgets: Vec<Widget>,
    /// Projects the page leaves out.
    pub hidden_projects: Vec<String>,
    /// The name the page shows for a project, by the project's own name.
    pub project_names: std::collections::BTreeMap<String, String>,
}

/// Why a document was refused: where, as a JSON pointer, and what is wrong.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Violation {
    pub place: String,
    pub message: String,
}

impl std::fmt::Display for Violation {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let place = if self.place.is_empty() {
            "the document"
        } else {
            &self.place
        };
        write!(f, "{place}: {}", self.message)
    }
}

impl std::error::Error for Violation {}

/// The schema of the dashboard base type.
#[must_use]
pub fn base_schema() -> Value {
    DashboardV1::<()>::gts_schema_with_refs()
}

/// The schema of the activity dashboard type.
#[must_use]
pub fn activity_schema() -> Value {
    ActivityDashboardV1::gts_schema_with_refs()
}

/// The schema a document of the activity type must pass, in one piece: the base type's schema and
/// the activity type's own, both of them, with no reference to follow.
#[must_use]
pub fn document_schema() -> Value {
    effective(&activity_schema(), &base_schema())
}

/// The schema a document of a derived type must pass: the schema of every type of its chain,
/// all of them. `schema` is the derived type's own, which refers to its base by `$ref`.
fn effective(schema: &Value, base: &Value) -> Value {
    json!({ "allOf": [base, schema_without_ref(schema)] })
}

/// The derived schema without its `$ref` to the base (the base is in the `allOf` beside it).
fn schema_without_ref(schema: &Value) -> Value {
    let mut own = schema.clone();
    if let Some(parts) = own.get_mut("allOf").and_then(Value::as_array_mut) {
        parts.retain(|part| part.get("$ref").is_none());
    }
    if let Some(object) = own.as_object_mut() {
        object.remove("$id");
        object.remove("$schema");
    }
    own
}

fn validator(schema: &Value, base: &Value) -> Result<Validator, Violation> {
    jsonschema::options()
        .with_draft(Draft::Draft7)
        .build(&effective(schema, base))
        .map_err(|e| Violation {
            place: String::new(),
            message: format!("the dashboard schema does not compile: {}", e.masked()),
        })
}

/// Checks `document` against the type `schema` of its kind, then against the rules a schema
/// cannot carry. Returns the first thing wrong.
///
/// # Errors
/// The document breaks the schema or one of the rules; the violation names the place.
pub fn validate(schema: &Value, document: &Value) -> Result<(), Violation> {
    // The base only checks that `type` is some derived type: the document must name this one.
    let own = schema["$id"]
        .as_str()
        .and_then(|id| id.strip_prefix("gts://"));
    if document["type"].as_str() != own {
        return Err(Violation {
            place: "/type".to_owned(),
            message: format!(
                "the document is not of the type {}",
                own.unwrap_or_default()
            ),
        });
    }
    if let Some(error) = validator(schema, &base_schema())?
        .iter_errors(document)
        .next()
    {
        return Err(Violation {
            place: error.instance_path().to_string(),
            message: error.masked().to_string(),
        });
    }
    check_rules(document)
}

/// [`validate`] for the activity dashboard.
///
/// # Errors
/// The document breaks the schema or one of the rules; the violation names the place.
pub fn validate_activity(document: &Value) -> Result<(), Violation> {
    validate(&activity_schema(), document)
}

/// The rules of the widgets that a schema cannot carry: a widget stays inside the grid, ids are
/// unique, only a table names what it groups by, and a widget asks only what its source can
/// answer.
fn check_rules(document: &Value) -> Result<(), Violation> {
    let Some(widgets) = document
        .pointer("/payload/widgets")
        .and_then(Value::as_array)
    else {
        return Ok(());
    };
    let page_source = document
        .pointer("/payload/source")
        .and_then(|v| serde_json::from_value::<Source>(v.clone()).ok());
    let mut seen = std::collections::BTreeSet::new();
    for (index, widget) in widgets.iter().enumerate() {
        let place = |member: &str| format!("/payload/widgets/{index}{member}");
        let violation = |member: &str, message: &str| Violation {
            place: place(member),
            message: message.to_owned(),
        };
        let number = |name: &str| {
            widget
                .pointer(&format!("/position/{name}"))
                .and_then(Value::as_u64)
                .unwrap_or(0)
        };
        if let Some(id) = widget.get("id").and_then(Value::as_str)
            && !seen.insert(id)
        {
            return Err(violation("/id", "the id is used by another widget"));
        }
        if number("column") + number("width") > u64::from(GRID_COLUMNS) {
            return Err(violation(
                "/position",
                "the widget does not fit in the 12 columns of the grid",
            ));
        }
        let is_table = widget.get("kind").and_then(Value::as_str) == Some("table");
        let grouped = widget
            .get("group_by")
            .is_some_and(|group_by| !group_by.is_null());
        if is_table && !grouped {
            return Err(violation("/group_by", "a table needs group_by"));
        }
        if !is_table && grouped {
            return Err(violation("/group_by", "only a table has group_by"));
        }
        let source = widget
            .get("source")
            .and_then(|v| serde_json::from_value::<Source>(v.clone()).ok())
            .or(page_source)
            .unwrap_or_default();
        let group_by = widget
            .get("group_by")
            .and_then(|v| serde_json::from_value::<GroupBy>(v.clone()).ok());
        if group_by.is_some_and(|group_by| !group_by.fits(source)) {
            return Err(violation(
                "/group_by",
                match source {
                    Source::AiSessions => "AI sessions are not grouped by app or site",
                    Source::OnScreen => "the time on screen is not grouped by tool or collection",
                },
            ));
        }
        if source == Source::OnScreen {
            if widget.get("kind").and_then(Value::as_str) == Some("day_summary") {
                return Err(violation(
                    "/kind",
                    "the summary of the day is of AI sessions only",
                ));
            }
            if widget.get("sessions").is_some_and(|v| !v.is_null()) {
                return Err(violation(
                    "/sessions",
                    "the time on screen has no sessions to choose between",
                ));
            }
            if widget
                .pointer("/filter/tools")
                .is_some_and(|tools| !tools.is_null())
            {
                return Err(violation(
                    "/filter/tools",
                    "the time on screen has no tools to filter by",
                ));
            }
        }
    }
    Ok(())
}

fn default_widget(id: &str, kind: &str, group_by: Option<&str>, at: [u32; 4]) -> Value {
    let mut widget = json!({
        "id": id,
        "kind": kind,
        "position": { "column": at[0], "row": at[1], "width": at[2], "height": at[3] },
    });
    if let (Some(group_by), Some(object)) = (group_by, widget.as_object_mut()) {
        object.insert("group_by".to_owned(), json!(group_by));
    }
    widget
}

/// The document written at the first start: this week, the user's own sessions, a total, bars
/// per day, a table by project, a table by ticket and the summary of the end of the day.
#[must_use]
pub fn default_activity() -> Value {
    json!({
        "type": ACTIVITY_TYPE,
        "title": "Activity",
        "payload": {
            "period": "this_week",
            "sessions": "own",
            "widgets": [
                default_widget("total", "total", None, [0, 0, 12, 2]),
                default_widget("per-day", "bars_per_day", None, [0, 2, 12, 4]),
                default_widget("by-project", "table", Some("project"), [0, 6, 6, 5]),
                default_widget("by-ticket", "table", Some("ticket"), [6, 6, 6, 5]),
                default_widget("end-of-day", "day_summary", None, [0, 11, 12, 3]),
            ],
            "hidden_projects": [],
            "project_names": {},
        },
    })
}

/// [`default_activity`] with the time on screen below it: a total, bars per day and a table by
/// application, by site and by ticket. The document of a computer that collects the screen.
#[must_use]
pub fn default_activity_with_screen() -> Value {
    let mut document = default_activity();
    let screen = [
        default_widget("screen-total", "total", None, [0, 14, 12, 2]),
        default_widget("screen-per-day", "bars_per_day", None, [0, 16, 12, 4]),
        default_widget("screen-by-app", "table", Some("app"), [0, 20, 4, 5]),
        default_widget("screen-by-site", "table", Some("site"), [4, 20, 4, 5]),
        default_widget("screen-by-ticket", "table", Some("ticket"), [8, 20, 4, 5]),
    ];
    if let Some(widgets) = document
        .pointer_mut("/payload/widgets")
        .and_then(Value::as_array_mut)
    {
        for mut widget in screen {
            widget["source"] = json!("on_screen");
            widgets.push(widget);
        }
    }
    document
}

fn write_document(workspace: &Workspace, document: &Value) -> Result<(), Error> {
    let mut text = serde_json::to_string_pretty(document).map_err(|_| Error::NotText)?;
    text.push('\n');
    workspace.write(ACTIVITY_FILE, &text)
}

/// Writes the default activity dashboard into the workspace when there is none. A file that is
/// there is the user's (or the chat's) and is left as it is. Returns whether it was written.
///
/// # Errors
/// The workspace refuses to read or write the file.
pub fn ensure_default(workspace: &Workspace) -> Result<bool, Error> {
    ensure_default_for(workspace, false)
}

/// [`ensure_default`], with the time on screen in the default when `screen` is set: the person
/// collects it.
///
/// # Errors
/// The workspace refuses to read or write the file.
pub fn ensure_default_for(workspace: &Workspace, screen: bool) -> Result<bool, Error> {
    match workspace.read(ACTIVITY_FILE, None, Some(1)) {
        Ok(_) => return Ok(false),
        Err(Error::NotFound) => {}
        Err(e) => return Err(e),
    }
    write_document(
        workspace,
        &if screen {
            default_activity_with_screen()
        } else {
            default_activity()
        },
    )?;
    Ok(true)
}

/// Puts the time on screen into the dashboard when the file is still the default as it was
/// written without it. A document that differs in anything, or that is not a document, is the
/// person's (or the chat's) and is not touched. Returns whether the file was written.
///
/// # Errors
/// The workspace refuses to read or write the file.
pub fn offer_screen(workspace: &Workspace, max_bytes: u64) -> Result<bool, Error> {
    let text = match workspace.read_whole(ACTIVITY_FILE, max_bytes) {
        Ok(text) => text,
        Err(Error::NotFound) => return Ok(false),
        Err(e) => return Err(e),
    };
    if serde_json::from_str::<Value>(&text).ok() != Some(default_activity()) {
        return Ok(false);
    }
    // A save that lands between the read above and the replacement below is written over: the
    // window is the time to parse a small document, and it cannot be closed without a lock on the
    // file, which the chat's file tools do not take, as for the edits of the dashboard.
    write_document(workspace, &default_activity_with_screen())?;
    Ok(true)
}
