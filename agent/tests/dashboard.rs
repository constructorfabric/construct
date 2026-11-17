//! The dashboard types and document: what is registered, what a document must pass, what is
//! refused and where, and the default document the agent writes.

#![cfg(unix)]
#![allow(clippy::unwrap_used)] // helpers outside #[test] functions use it too

use construct_agent::dashboard::{
    ACTIVITY_FILE, ACTIVITY_TYPE, ActivityDashboardV1, BASE_TYPE, DashboardV1, GRID_COLUMNS,
    activity_schema, default_activity, ensure_default, validate, validate_activity,
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
    let typed: DashboardV1<ActivityDashboardV1> = serde_json::from_value(document.clone()).unwrap();
    assert_eq!(typed.payload.widgets.len(), 5);
    assert_eq!(serde_json::to_value(&typed).unwrap(), document);
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
