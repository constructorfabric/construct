//! What the agent itself keeps about the person, and the one list that `memory-erase` goes
//! through. The host's copy is erased by the host (see `manage`); this is the agent's own.
//!
//! Every store clears itself ([`OwnStore`]). **A store that a later change adds, because it
//! keeps something about the person, joins the list in [`own_stores`], with one line (its handle
//! is one more argument there, given by `main`).** Its
//! `erase` must leave nothing of it on the disk or in memory, and must touch no file of the
//! tools whose sessions are read (the Claude Code and Codex folders): only the agent's own files.
//!
//! The model and the settings are not on the list: they are not about the person's work.

use std::sync::Arc;

/// One place where the agent keeps something about the person.
pub trait OwnStore: Send + Sync {
    /// What the store holds, in plain words, for the answer to a failed erasure.
    fn name(&self) -> &'static str;

    /// Removes everything the store holds, from the disk and from memory. Collection is off
    /// while this runs, so nothing new is written; the store waits for what is writing now.
    ///
    /// # Errors
    /// The reason it could not, in plain words and without a path or a text.
    fn erase(&self) -> Result<(), String>;
}

/// The stores in the order they are erased.
pub struct OwnStores(Vec<Arc<dyn OwnStore>>);

impl OwnStores {
    #[must_use]
    pub fn new(stores: Vec<Arc<dyn OwnStore>>) -> Self {
        Self(stores)
    }

    /// Erases every store, one after the other; a store that fails does not stop the rest.
    /// Returns the names of the stores that could not be erased.
    #[must_use]
    pub fn erase(&self) -> Vec<&'static str> {
        self.0
            .iter()
            .filter(|store| match store.erase() {
                Ok(()) => false,
                Err(reason) => {
                    // The reason is a fixed plain text: no path, no content.
                    eprintln!("erase: {}: {reason}", store.name());
                    true
                }
            })
            .map(|store| store.name())
            .collect()
    }
}

/// THE LIST of the agent's own stores. A new store joins it with one line below, and its handle
/// is passed in as an argument (the caller is `main`; the tests build the same list).
///
/// The order matters in one place: the activity time is made from the record files, so it goes
/// after them.
#[cfg(unix)]
#[must_use]
pub fn own_stores(
    sessions: &crate::follow::Erase,
    chat: Option<&crate::chat::Chat>,
    manage: &crate::manage::Manager,
    activity: &crate::activity::Feed,
    setup: Option<&crate::dashboard_setup::SetupJob>,
    runtime: &tokio::runtime::Handle,
) -> OwnStores {
    let mut stores: Vec<Arc<dyn OwnStore>> =
        vec![Arc::new(sessions.clone()), manage.journal_store()];
    if let Some(chat) = chat {
        stores.push(Arc::new(chat.clone()));
    }
    if let Some(setup) = setup {
        stores.push(Arc::new(setup.clone()));
    }
    stores.push(Arc::new(crate::activity::ActivityStore::new(
        activity, runtime,
    )));
    OwnStores::new(stores)
}
