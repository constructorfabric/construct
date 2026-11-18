//! What the person changed in the dashboard since the last run, put into the page a run wrote.
//!
//! A run starts from the dashboard as it is, and the session may rewrite the whole file. What the
//! person changed is what differs from the dashboard the last finished run left (the baseline).
//! Before a first run there is none, and the baseline is the default dashboard, the one the agent
//! writes when there is no file: what the person did to it, in the page or through the chat,
//! before their first setup is theirs too.

use serde_json::{Map, Value, json};

/// The hidden projects and the names of a document, as its payload keeps them.
fn marks(document: &Value) -> (Vec<Value>, Map<String, Value>) {
    let payload = document.get("payload");
    let hidden = payload
        .and_then(|p| p.get("hidden_projects"))
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let names = payload
        .and_then(|p| p.get("project_names"))
        .and_then(Value::as_object)
        .cloned()
        .unwrap_or_default();
    (hidden, names)
}

fn widgets(document: &Value) -> Vec<Value> {
    document
        .get("payload")
        .and_then(|p| p.get("widgets"))
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default()
}

fn id_of(widget: &Value) -> Option<&str> {
    widget.get("id").and_then(Value::as_str)
}

/// Puts the person's hidden and shown projects and names into `payload`.
fn keep_marks(payload: &mut Map<String, Value>, now: &Value, base: &Value) -> Option<()> {
    let ((hidden_now, names_now), (hidden_base, names_base)) = (marks(now), marks(base));
    let hidden = payload
        .entry("hidden_projects")
        .or_insert_with(|| json!([]))
        .as_array_mut()?;
    for project in &hidden_now {
        if !hidden_base.contains(project) && !hidden.contains(project) {
            hidden.push(project.clone());
        }
    }
    hidden.retain(|project| !hidden_base.contains(project) || hidden_now.contains(project));
    let names = payload
        .entry("project_names")
        .or_insert_with(|| Value::Object(Map::new()))
        .as_object_mut()?;
    for (project, name) in &names_now {
        if names_base.get(project) != Some(name) {
            names.insert(project.clone(), name.clone());
        }
    }
    for project in names_base.keys() {
        if !names_now.contains_key(project) {
            names.remove(project);
        }
    }
    Some(())
}

/// Puts the widgets the person added or changed into `payload`, and takes out those they removed.
fn keep_widgets(payload: &mut Map<String, Value>, now: &Value, base: &Value) -> Option<()> {
    let (now, base) = (widgets(now), widgets(base));
    let list = payload.get_mut("widgets")?.as_array_mut()?;
    for widget in &now {
        let Some(id) = id_of(widget) else { continue };
        if base.iter().any(|old| old == widget) {
            continue;
        }
        match list.iter().position(|w| id_of(w) == Some(id)) {
            Some(at) => list[at] = widget.clone(),
            None => list.push(widget.clone()),
        }
    }
    for old in &base {
        let Some(id) = id_of(old) else { continue };
        if !now.iter().any(|w| id_of(w) == Some(id)) {
            list.retain(|w| id_of(w) != Some(id));
        }
    }
    Some(())
}

/// `after`, the dashboard the session wrote, with what the person changed in `now`, the dashboard
/// as it was when the run began, since `baseline`, the dashboard the last run left: a project they
/// hid stays hidden, one they showed again stays shown, a name they gave or took away stays so,
/// and, when `with_widgets` is set, a widget they added or changed stays and one they removed
/// stays removed. `None` when there is nothing to change, or when `now` is not a document with a
/// payload. Without a `baseline` it is the default dashboard.
pub(super) fn keep_changes(
    now: &str,
    baseline: Option<&str>,
    after: &Value,
    with_widgets: bool,
) -> Option<Value> {
    let now: Value = serde_json::from_str(now).ok()?;
    now.get("payload")?;
    let base: Value = baseline
        .and_then(|text| serde_json::from_str(text).ok())
        .unwrap_or_else(crate::dashboard::default_activity);
    let mut kept = after.clone();
    let payload = kept.get_mut("payload")?.as_object_mut()?;
    keep_marks(payload, &now, &base)?;
    if with_widgets {
        keep_widgets(payload, &now, &base)?;
    }
    (kept != *after).then_some(kept)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn document(hidden: &[&str], names: &Value) -> Value {
        json!({ "payload": { "hidden_projects": hidden, "project_names": names, "widgets": [] } })
    }

    #[test]
    fn what_the_person_changed_since_the_last_run_is_put_back() {
        let baseline = document(&["/w/lab"], &json!({ "/w/shop": "PLAT" })).to_string();
        // The person hid the blog, showed the lab again, named the shop, and named the blog.
        let now = document(
            &["/w/blog"],
            &json!({ "/w/shop": "My shop", "/w/blog": "Blog" }),
        );
        let after = document(&["/w/lab", "/w/old"], &json!({ "/w/shop": "PLAT" }));
        let kept = keep_changes(&now.to_string(), Some(&baseline), &after, true).unwrap();
        assert_eq!(
            kept["payload"]["hidden_projects"],
            json!(["/w/old", "/w/blog"])
        );
        assert_eq!(
            kept["payload"]["project_names"],
            json!({ "/w/shop": "My shop", "/w/blog": "Blog" })
        );
    }

    #[test]
    fn a_name_the_session_gave_last_time_is_the_sessions_to_change() {
        let baseline = document(&[], &json!({ "/w/shop": "PLAT" })).to_string();
        let after = document(&[], &json!({ "/w/shop": "Shop again" }));
        assert_eq!(keep_changes(&baseline, Some(&baseline), &after, true), None);
    }

    #[test]
    fn without_a_baseline_everything_on_the_page_is_the_persons() {
        let now = document(&["/w/lab"], &json!({ "/w/shop": "My shop" })).to_string();
        let after = document(&[], &json!({}));
        let kept = keep_changes(&now, None, &after, true).unwrap();
        assert_eq!(kept["payload"]["hidden_projects"], json!(["/w/lab"]));
        assert_eq!(
            kept["payload"]["project_names"],
            json!({ "/w/shop": "My shop" })
        );
        assert_eq!(keep_changes("not json", None, &after, true), None);
    }

    fn widget(id: &str, row: u32) -> Value {
        json!({ "id": id, "kind": "total",
            "position": { "column": 0, "row": row, "width": 12, "height": 2 } })
    }

    fn with_widgets(list: &[Value]) -> Value {
        let mut document = document(&[], &json!({}));
        document["payload"]["widgets"] = json!(list);
        document
    }

    #[test]
    fn a_widget_the_person_added_changed_or_removed_stays_so() {
        let baseline = with_widgets(&[widget("a", 0), widget("b", 2), widget("c", 4)]);
        // The person changed `b`, removed `c` and added `mine`.
        let now = with_widgets(&[widget("a", 0), widget("b", 9), widget("mine", 6)]);
        // The session left `a`, moved `c`, and added `new`.
        let after = with_widgets(&[
            widget("a", 0),
            widget("b", 2),
            widget("c", 5),
            widget("new", 8),
        ]);
        let kept =
            keep_changes(&now.to_string(), Some(&baseline.to_string()), &after, true).unwrap();
        let ids: Vec<&str> = kept["payload"]["widgets"]
            .as_array()
            .unwrap()
            .iter()
            .map(|w| w["id"].as_str().unwrap())
            .collect();
        assert_eq!(ids, ["a", "b", "new", "mine"]);
        assert_eq!(kept["payload"]["widgets"][1]["position"]["row"], 9);
        // Asked not to, the widgets are the session's.
        assert_eq!(
            keep_changes(&now.to_string(), Some(&baseline.to_string()), &after, false),
            None
        );
    }

    #[test]
    fn before_a_first_run_the_default_page_is_the_baseline() {
        let mut now = crate::dashboard::default_activity();
        // The person added a widget and hid a project, and left the default widgets as they are.
        now["payload"]["widgets"]
            .as_array_mut()
            .unwrap()
            .push(widget("mine", 14));
        now["payload"]["hidden_projects"] = json!(["/w/lab"]);
        let after = with_widgets(&[widget("new", 0)]);
        let kept = keep_changes(&now.to_string(), None, &after, true).unwrap();
        let ids: Vec<&str> = kept["payload"]["widgets"]
            .as_array()
            .unwrap()
            .iter()
            .map(|w| w["id"].as_str().unwrap())
            .collect();
        // The default widgets, which the person did not change, are not put back.
        assert_eq!(ids, ["new", "mine"]);
        assert_eq!(kept["payload"]["hidden_projects"], json!(["/w/lab"]));
    }
}
