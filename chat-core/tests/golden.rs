//! The request body of a fixed conversation with a `recall_facts` pair, built over the tool file of
//! the phone and over the tool file of the desktop, against stored bodies. The bytes are what the
//! provider's prompt cache sees: a change to a tool text, to the order, or to the way the body is
//! written must show here, and is made on purpose or not at all.
#![allow(clippy::unwrap_used)] // helpers outside #[test] functions use it too

use chat_core::fixture::request_body_over;

/// A stored body without the newline that the end-of-file check of the repository puts at the end
/// of a file: the body itself has none.
fn stored(file: &str) -> &str {
    file.strip_suffix('\n').unwrap_or(file)
}

#[test]
fn the_phone_list_gives_the_stored_body() {
    let body = request_body_over(include_str!("../../tools/phone.json")).unwrap();
    assert_eq!(body, stored(include_str!("golden/phone.json")));
    // The phone offers `recall_facts` and nothing else.
    assert_eq!(body.matches("\"parameters\":").count(), 1);
}

#[test]
fn the_desktop_list_gives_the_stored_body() {
    let body = request_body_over(include_str!("../../tools/desktop.json")).unwrap();
    assert_eq!(body, stored(include_str!("golden/desktop.json")));
}
