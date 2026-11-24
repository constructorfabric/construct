//! The agent's own source for the sync commands: the facts from the Construct host, the chats of
//! the chat, the dashboard document and the activity store.

use std::time::Duration;

use serde_json::Value;

use super::sync::{ChatRead, Data, FactsRead, Reason, chat_read};
use crate::chat::{Chat, Link};
use crate::dashboard::Source;
use crate::dashboard_document::Dashboard;
use crate::dashboard_numbers::{Data as Activity, Numbers};
use crate::host::loopback_agent;

const TIMEOUT: Duration = Duration::from_secs(5);
const PAGE: usize = 100;
/// The most pages of facts read in one go; a person with more is not copied whole.
const MAX_PAGES: usize = 200;

/// What the agent has to give.
pub struct Live {
    pub chat: Chat,
    pub link: Link,
    pub dashboard: Option<Dashboard>,
    pub numbers: Option<Numbers>,
}

impl Live {
    fn page(&self, query: &str) -> Result<Value, Reason> {
        let connection = self.link.get().ok_or("facts unavailable")?;
        let mut response = loopback_agent(TIMEOUT)
            .get(&connection.subject_url(&format!("facts?{query}")))
            .header("Authorization", format!("Bearer {}", connection.token))
            .call()
            .map_err(|_| "facts unavailable")?;
        if response.status().as_u16() != 200 {
            return Err("facts unavailable");
        }
        let text = response
            .body_mut()
            .read_to_string()
            .map_err(|_| "facts unavailable")?;
        serde_json::from_str(&text).map_err(|_| "facts unavailable")
    }
}

fn version_of(body: &Value) -> Option<u64> {
    body.get("version").and_then(Value::as_u64)
}

impl Data for Live {
    fn facts(&self) -> Result<FactsRead, Reason> {
        let mut read = FactsRead::default();
        let mut cursor: Option<String> = None;
        for page in 0..MAX_PAGES {
            let query = match &cursor {
                None => format!("limit={PAGE}"),
                Some(c) => format!(
                    "limit={PAGE}&cursor={}",
                    url::form_urlencoded::byte_serialize(c.as_bytes()).collect::<String>()
                ),
            };
            let body = self.page(&query)?;
            if page == 0 {
                read.version = version_of(&body);
            }
            read.facts.extend(
                body.get("items")
                    .and_then(Value::as_array)
                    .cloned()
                    .unwrap_or_default(),
            );
            cursor = body
                .get("page_info")
                .and_then(|p| p.get("next_cursor"))
                .and_then(Value::as_str)
                .map(str::to_owned);
            if cursor.is_none() {
                return Ok(read);
            }
        }
        Err("facts unavailable")
    }

    fn facts_version(&self) -> Result<Option<u64>, Reason> {
        Ok(version_of(&self.page("limit=1")?))
    }

    fn chats(&self) -> Result<Vec<ChatRead>, Reason> {
        let stored = self.chat.conversations().map_err(|_| "chats unavailable")?;
        let line = |name: &str, arguments: &str| self.chat.tool_line(name, arguments);
        Ok(stored
            .into_iter()
            .filter_map(|(id, records)| chat_read(id, &records, &line))
            .collect())
    }

    fn document(&self) -> Result<Option<Value>, Reason> {
        let Some(dashboard) = &self.dashboard else {
            return Ok(None);
        };
        let document = dashboard.to_json()["document"].clone();
        Ok((!document.is_null()).then_some(document))
    }

    fn activity(&self, source: Source) -> Result<Activity, Reason> {
        self.numbers
            .as_ref()
            .ok_or("the activity time could not be read")?
            .read_data(source)
    }
}
