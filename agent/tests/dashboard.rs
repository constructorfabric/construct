//! The dashboard types and document: what is registered, what a document must pass, what is
//! refused and where, and the default document the agent writes.

#![cfg(unix)]
#![allow(clippy::unwrap_used)] // helpers outside #[test] functions use it too

use construct_agent::dashboard::{
    ACTIVITY_FILE, ACTIVITY_TYPE, ACTIVITY_TYPE_V1, ActivityDashboardV2, BASE_TYPE, DashboardV1,
    GRID_COLUMNS, activity_schema, base_schema, default_activity, default_activity_with_screen,
    ensure_default, ensure_default_for, offer_screen, validate, validate_activity,
};
use construct_agent::workspace::Workspace;
use gts::GtsSchema;
use serde_json::{Value, json};
use toolkit_gts::{gts_id, gts_type_schema};

/// A second kind of dashboard, added next to the activity one.
#[derive(Debug, Clone, PartialEq, Eq)]
#[gts_type_schema(
    dir_path = "schemas",
    base = DashboardV1,
    type_id = gts_id!("cf.construct.core.dashboard.v1~cf.construct.dashboard.students.v1~"),
    description = "A dashboard of the courses a student follows.",
    properties = "courses"
)]
pub struct StudentDashboardV1 {
    pub courses: Vec<String>,
}

const STUDENT_TYPE: &str =
    gts_id!("cf.construct.core.dashboard.v1~cf.construct.dashboard.students.v1~");

fn refused(document: &Value) -> (String, String) {
    let violation = validate_activity(document).unwrap_err();
    (violation.place, violation.message)
}

/// The default document with `change` applied.
fn changed(change: impl FnOnce(&mut Value)) -> Value {
    let mut document = default_activity();
    change(&mut document);
    document
}

fn temp_workspace(name: &str) -> (std::path::PathBuf, Workspace) {
    let dir = std::env::temp_dir().join(format!("cb-dashboard-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let workspace = Workspace::open(&dir).unwrap();
    (dir, workspace)
}

#[test]
fn the_types_are_in_the_link_time_inventory() {
    let schemas = toolkit_gts::all_inventory_type_schemas().unwrap();
    let ids: Vec<&str> = schemas
        .iter()
        .filter_map(|schema| schema["$id"].as_str())
        .collect();
    assert!(
        ids.contains(&format!("gts://{BASE_TYPE}").as_str()),
        "{ids:?}"
    );
    assert!(
        ids.contains(&format!("gts://{ACTIVITY_TYPE}").as_str()),
        "{ids:?}"
    );
    assert_eq!(activity_schema()["$id"], format!("gts://{ACTIVITY_TYPE}"));
}

#[test]
fn the_default_document_validates_and_reads_into_the_type() {
    let document = default_activity();
    validate_activity(&document).unwrap();
    // The type reads every member as present: a document with no source is read with a null one.
    let mut read = document.clone();
    read["payload"]["source"] = Value::Null;
    let typed: DashboardV1<ActivityDashboardV2> = serde_json::from_value(read.clone()).unwrap();
    assert_eq!(typed.payload.widgets.len(), 5);
    assert_eq!(typed.payload.source, None);
    assert_eq!(serde_json::to_value(&typed).unwrap(), read);
}

#[test]
fn the_default_document_is_this_week_of_own_sessions_laid_out_on_the_grid() {
    let document = default_activity();
    assert_eq!(document["payload"]["period"], "this_week");
    assert_eq!(document["payload"]["sessions"], "own");
    let kinds: Vec<_> = document["payload"]["widgets"]
        .as_array()
        .unwrap()
        .iter()
        .map(|w| (w["kind"].as_str().unwrap(), w["group_by"].as_str()))
        .collect();
    assert_eq!(
        kinds,
        [
            ("total", None),
            ("bars_per_day", None),
            ("table", Some("project")),
            ("table", Some("ticket")),
            ("day_summary", None),
        ]
    );
    for widget in document["payload"]["widgets"].as_array().unwrap() {
        let at = &widget["position"];
        assert!(
            at["column"].as_u64().unwrap() + at["width"].as_u64().unwrap()
                <= u64::from(GRID_COLUMNS)
        );
    }
}

#[test]
fn a_document_with_every_option_validates() {
    let document = changed(|d| {
        d["payload"]["hidden_projects"] = json!(["scratch"]);
        d["payload"]["project_names"] = json!({ "website": "Platform" });
        d["payload"]["widgets"][2]["period"] = json!("last_30_days");
        d["payload"]["widgets"][2]["sessions"] = json!("all");
        d["payload"]["widgets"][2]["filter"] =
            json!({ "projects": ["website"], "tools": ["claude"] });
    });
    validate_activity(&document).unwrap();
}

#[test]
fn a_refusal_names_the_place() {
    let cases: Vec<(Value, &str)> = vec![
        (
            changed(|d| d["payload"]["widgets"][1]["kind"] = json!("pie")),
            "/payload/widgets/1/kind",
        ),
        (
            changed(|d| d["payload"]["period"] = json!("fortnight")),
            "/payload/period",
        ),
        (
            changed(|d| d["payload"]["widgets"][3]["period"] = json!("last_decade")),
            "/payload/widgets/3/period",
        ),
        (
            changed(|d| {
                d["payload"].as_object_mut().unwrap().remove("sessions");
            }),
            "/payload",
        ),
        (
            changed(|d| {
                d["payload"]["widgets"][0]
                    .as_object_mut()
                    .unwrap()
                    .remove("position");
            }),
            "/payload/widgets/0",
        ),
        (
            changed(|d| d["payload"]["widgets"][0]["script"] = json!("1+1")),
            "/payload/widgets/0",
        ),
        (
            changed(|d| d["payload"]["widgets"][0]["position"]["width"] = json!(13)),
            "/payload/widgets/0/position/width",
        ),
        (
            changed(|d| d["payload"]["widgets"][4]["position"]["column"] = json!(6)),
            "/payload/widgets/4/position",
        ),
        (
            changed(|d| d["payload"]["widgets"][4]["id"] = json!("total")),
            "/payload/widgets/4/id",
        ),
        (
            changed(|d| {
                d["payload"]["widgets"][2]
                    .as_object_mut()
                    .unwrap()
                    .remove("group_by");
            }),
            "/payload/widgets/2/group_by",
        ),
        (
            changed(|d| d["payload"]["widgets"][0]["group_by"] = json!("tool")),
            "/payload/widgets/0/group_by",
        ),
        (changed(|d| d["title"] = json!("")), "/title"),
    ];
    for (document, place) in cases {
        let (found, message) = refused(&document);
        assert_eq!(found, place, "{message}");
    }
}

#[test]
fn a_missing_member_is_named_in_the_message() {
    let document = changed(|d| {
        d["payload"].as_object_mut().unwrap().remove("sessions");
    });
    assert!(refused(&document).1.contains("sessions"));
}

#[test]
fn a_document_of_another_kind_is_refused_by_its_type() {
    let other = changed(|d| d["type"] = json!(STUDENT_TYPE));
    assert_eq!(validate_activity(&other).unwrap_err().place, "/type");
    let missing = changed(|d| {
        d.as_object_mut().unwrap().remove("type");
    });
    assert_eq!(validate_activity(&missing).unwrap_err().place, "/type");
}

#[test]
fn a_derived_type_can_be_added_next_to_the_activity_one() {
    let ids: Vec<String> = toolkit_gts::all_inventory_type_schemas()
        .unwrap()
        .iter()
        .filter_map(|schema| schema["$id"].as_str().map(str::to_owned))
        .collect();
    assert!(ids.contains(&format!("gts://{STUDENT_TYPE}")));

    let schema = StudentDashboardV1::gts_schema_with_refs();
    let student = json!({
        "type": STUDENT_TYPE,
        "title": "My courses",
        "payload": { "courses": ["Rust"] },
    });
    validate(&schema, &student).unwrap();
    let typed: DashboardV1<StudentDashboardV1> = serde_json::from_value(student).unwrap();
    assert_eq!(typed.payload.courses, ["Rust"]);

    // Each kind keeps its own payload: neither schema takes the other's document.
    assert_eq!(
        validate(&schema, &default_activity()).unwrap_err().place,
        "/type"
    );
    let named =
        json!({ "type": STUDENT_TYPE, "title": "x", "payload": default_activity()["payload"] });
    assert_eq!(validate(&schema, &named).unwrap_err().place, "/payload");
    let wrong = json!({ "type": ACTIVITY_TYPE, "title": "x", "payload": { "courses": [] } });
    assert_eq!(validate_activity(&wrong).unwrap_err().place, "/payload");
}

#[test]
fn the_default_document_is_written_once_and_a_changed_one_is_kept() {
    let (dir, workspace) = temp_workspace("once");
    assert!(ensure_default(&workspace).unwrap());
    let text = workspace
        .read(ACTIVITY_FILE, None, None)
        .unwrap()
        .join("\n");
    let written: Value = serde_json::from_str(&text).unwrap();
    assert_eq!(written, default_activity());
    validate_activity(&written).unwrap();

    let mine = changed(|d| d["title"] = json!("Mine")).to_string();
    workspace.write(ACTIVITY_FILE, &mine).unwrap();
    assert!(!ensure_default(&workspace).unwrap());
    assert_eq!(
        workspace
            .read(ACTIVITY_FILE, None, None)
            .unwrap()
            .join("\n"),
        mine
    );
    std::fs::remove_dir_all(dir).unwrap();
}

fn read_file(workspace: &Workspace) -> Value {
    serde_json::from_str(
        &workspace
            .read(ACTIVITY_FILE, None, None)
            .unwrap()
            .join("\n"),
    )
    .unwrap()
}

#[test]
fn the_default_with_the_screen_adds_a_total_bars_and_tables_by_app_site_and_ticket() {
    let document = default_activity_with_screen();
    validate_activity(&document).unwrap();
    let widgets = document["payload"]["widgets"].as_array().unwrap();
    let screen: Vec<(&str, &str, Option<&str>)> = widgets
        .iter()
        .filter(|w| w["source"] == "on_screen")
        .map(|w| {
            (
                w["id"].as_str().unwrap(),
                w["kind"].as_str().unwrap(),
                w["group_by"].as_str(),
            )
        })
        .collect();
    assert_eq!(
        screen,
        [
            ("screen-total", "total", None),
            ("screen-per-day", "bars_per_day", None),
            ("screen-by-app", "table", Some("app")),
            ("screen-by-site", "table", Some("site")),
            ("screen-by-ticket", "table", Some("ticket")),
        ]
    );
    // The sessions stay as they were, above the time on screen, and no widget overlaps another.
    assert_eq!(
        &widgets[..5],
        default_activity()["payload"]["widgets"].as_array().unwrap()
    );
    let cells: Vec<(u64, u64, u64, u64)> = widgets
        .iter()
        .map(|w| {
            let p = &w["position"];
            (
                p["column"].as_u64().unwrap(),
                p["row"].as_u64().unwrap(),
                p["width"].as_u64().unwrap(),
                p["height"].as_u64().unwrap(),
            )
        })
        .collect();
    for (i, a) in cells.iter().enumerate() {
        for b in &cells[i + 1..] {
            let apart =
                a.0 + a.2 <= b.0 || b.0 + b.2 <= a.0 || a.1 + a.3 <= b.1 || b.1 + b.3 <= a.1;
            assert!(apart, "{a:?} overlaps {b:?}");
        }
    }
}

#[test]
fn the_default_document_holds_the_screen_only_while_it_is_collected() {
    let (dir, off) = temp_workspace("screen-off");
    assert!(ensure_default_for(&off, false).unwrap());
    assert_eq!(read_file(&off), default_activity());
    let (_, on) = temp_workspace("screen-on");
    assert!(ensure_default_for(&on, true).unwrap());
    assert_eq!(read_file(&on), default_activity_with_screen());
    // A file that is there is not written over, whatever is collected.
    assert!(!ensure_default_for(&off, true).unwrap());
    assert_eq!(read_file(&off), default_activity());
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn the_screen_is_offered_to_the_default_document_and_to_no_other() {
    let (dir, workspace) = temp_workspace("offer");
    // No file: nothing to offer to.
    assert!(!offer_screen(&workspace, 1 << 20).unwrap());
    ensure_default(&workspace).unwrap();
    assert!(offer_screen(&workspace, 1 << 20).unwrap());
    assert_eq!(read_file(&workspace), default_activity_with_screen());
    // It is offered once: the document now holds it.
    assert!(!offer_screen(&workspace, 1 << 20).unwrap());

    // The default as an earlier build wrote it (the first type) is the same document: it gets the
    // screen too, and has the current type afterwards.
    let mut first = default_activity();
    first["type"] = json!(ACTIVITY_TYPE_V1);
    workspace.write(ACTIVITY_FILE, &first.to_string()).unwrap();
    assert!(offer_screen(&workspace, 1 << 20).unwrap());
    assert_eq!(read_file(&workspace), default_activity_with_screen());
    // An earlier default that the person changed is theirs, whatever its type.
    first["title"] = json!("Mine");
    workspace.write(ACTIVITY_FILE, &first.to_string()).unwrap();
    assert!(!offer_screen(&workspace, 1 << 20).unwrap());

    // A document the person or the chat has changed is not touched, however small the change.
    let edits: [fn(&mut Value); 6] = [
        |d| d["title"] = json!("Mine"),
        |d| d["payload"]["period"] = json!("today"),
        |d| d["payload"]["hidden_projects"] = json!(["scratch"]),
        |d| d["payload"]["project_names"] = json!({ "a": "b" }),
        |d| {
            d["payload"]["widgets"].as_array_mut().unwrap().pop();
        },
        |d| d["payload"]["widgets"][0]["position"]["height"] = json!(3),
    ];
    for edit in edits {
        let mine = changed(edit).to_string();
        workspace.write(ACTIVITY_FILE, &mine).unwrap();
        assert!(!offer_screen(&workspace, 1 << 20).unwrap());
        assert_eq!(
            workspace
                .read(ACTIVITY_FILE, None, None)
                .unwrap()
                .join("\n"),
            mine
        );
    }
    // A file that is not a document is the person's too.
    workspace.write(ACTIVITY_FILE, "{ not json").unwrap();
    assert!(!offer_screen(&workspace, 1 << 20).unwrap());
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn a_document_with_no_source_is_valid_and_one_with_a_source_names_a_kind() {
    let plain = default_activity();
    assert!(plain["payload"].get("source").is_none());
    validate_activity(&plain).unwrap();
    for source in ["ai_sessions", "on_screen"] {
        let mut page = plain.clone();
        page["payload"]["source"] = json!(source);
        // The summary of the day is of AI sessions, so a page of the screen says so on it.
        page["payload"]["widgets"][4]["source"] = json!("ai_sessions");
        validate_activity(&page).unwrap();
    }
    let mut bad = plain.clone();
    bad["payload"]["source"] = json!("everything");
    assert_eq!(
        validate_activity(&bad).unwrap_err().place,
        "/payload/source"
    );
    let mut bad = plain;
    bad["payload"]["widgets"][0]["source"] = json!("screen");
    assert_eq!(
        validate_activity(&bad).unwrap_err().place,
        "/payload/widgets/0/source"
    );
}

#[test]
fn a_widget_asks_only_what_its_source_can_answer() {
    let table = |source: &str, group_by: &str| {
        changed(|d| {
            d["payload"]["widgets"][2]["source"] = json!(source);
            d["payload"]["widgets"][2]["group_by"] = json!(group_by);
        })
    };
    for (source, group_by) in [
        ("ai_sessions", "project"),
        ("ai_sessions", "tool"),
        ("ai_sessions", "collection"),
        ("on_screen", "project"),
        ("on_screen", "ticket"),
        ("on_screen", "day"),
        ("on_screen", "app"),
        ("on_screen", "site"),
    ] {
        validate_activity(&table(source, group_by))
            .unwrap_or_else(|e| panic!("{source} {group_by}: {e}"));
    }
    for (source, group_by, message) in [
        (
            "ai_sessions",
            "app",
            "AI sessions are not grouped by app or site",
        ),
        (
            "ai_sessions",
            "site",
            "AI sessions are not grouped by app or site",
        ),
        (
            "on_screen",
            "tool",
            "the time on screen is not grouped by tool or collection",
        ),
        (
            "on_screen",
            "collection",
            "the time on screen is not grouped by tool or collection",
        ),
    ] {
        let (place, said) = refused(&table(source, group_by));
        assert_eq!(
            (place.as_str(), said.as_str()),
            ("/payload/widgets/2/group_by", message)
        );
    }
    // A page of the screen is the source of every widget that names none, the summary of the day
    // included, which is refused there.
    let page = changed(|d| d["payload"]["source"] = json!("on_screen"));
    assert_eq!(
        refused(&page),
        (
            "/payload/widgets/4/kind".to_owned(),
            "the summary of the day is of AI sessions only".to_owned()
        )
    );
    let sessions = changed(|d| {
        d["payload"]["widgets"][0]["source"] = json!("on_screen");
        d["payload"]["widgets"][0]["sessions"] = json!("all");
    });
    assert_eq!(
        refused(&sessions),
        (
            "/payload/widgets/0/sessions".to_owned(),
            "the time on screen has no sessions to choose between".to_owned()
        )
    );
    let tools = changed(|d| {
        d["payload"]["widgets"][0]["source"] = json!("on_screen");
        d["payload"]["widgets"][0]["filter"] = json!({ "tools": ["claude"] });
    });
    assert_eq!(
        refused(&tools),
        (
            "/payload/widgets/0/filter/tools".to_owned(),
            "the time on screen has no tools to filter by".to_owned()
        )
    );
}

/// The registry that holds a type refuses a new definition of it that it cannot prove accepts every
/// document the old one accepted, with the comparison of `gts::GtsStore::compare_documents`.
fn compare_with(earlier: &str) -> gts::SchemaComparison {
    let earlier: Value = serde_json::from_str(earlier).unwrap();
    let mut store = gts::GtsStore::new();
    store.register_schema(BASE_TYPE, &base_schema()).unwrap();
    store
        .compare_documents(&earlier, &activity_schema())
        .unwrap()
}

/// The schema as written for this version is a fixture. When this test fails, either change the
/// schema in a way the comparison can prove, or give the type a new major version, keep this file
/// and write the new schema as the fixture of that version.
#[test]
fn the_schema_accepts_every_document_the_one_as_written_accepted() {
    let earlier = include_str!("fixtures/dashboard-activity-v2.schema.json");
    assert_eq!(
        serde_json::from_str::<Value>(earlier).unwrap()["$id"],
        activity_schema()["$id"]
    );
    let comparison = compare_with(earlier);
    assert_eq!(
        comparison.backward_compatibility(),
        gts::CompatibilityVerdict::Compatible,
        "{:?}",
        comparison.backward_diagnostics
    );
}

/// The fault this version avoids: the schema of the first builds with `app` and `site` added to
/// `group_by` cannot be proved to accept the documents of the first, so the registry refuses it.
/// The comparison finds it, which shows the test above can fail.
#[test]
fn the_comparison_cannot_prove_the_new_groupings_over_the_first_schema() {
    let comparison = compare_with(include_str!("fixtures/dashboard-activity-v1.schema.json"));
    assert_eq!(
        comparison.backward_compatibility(),
        gts::CompatibilityVerdict::Unknown
    );
    let places: Vec<&str> = comparison
        .backward_diagnostics
        .iter()
        .map(|d| d.path.as_str())
        .collect();
    assert_eq!(places, ["$.payload.widgets[].group_by"]);
    assert_eq!(
        comparison.backward_diagnostics[0].finding,
        gts::CompatibilityFinding::NotProvable
    );
}

#[test]
fn a_document_of_the_first_type_is_still_valid() {
    // What the first builds wrote: no source, only the groupings of the sessions.
    let mut first = default_activity();
    first["type"] = json!(ACTIVITY_TYPE_V1);
    validate_activity(&first).unwrap();
    // It is still held to the schema: the place of a fault is named as for any document.
    first["payload"]["period"] = json!("tomorrow");
    assert_eq!(
        validate_activity(&first).unwrap_err().place,
        "/payload/period"
    );
    // Any other type is refused.
    let mut other = default_activity();
    other["type"] = json!(format!("{BASE_TYPE}cf.construct.dashboard.activity.v3~"));
    assert_eq!(validate_activity(&other).unwrap_err().place, "/type");
}
