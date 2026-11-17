//! What one call to the route carries: the prompt and the turns before it, cut to the route's limits.

use std::collections::VecDeque;

use connectors::{Role, Turn};

/// The longest prompt the route takes, in characters.
pub const MESSAGE_LIMIT: usize = 4_000;
/// The longest context the route takes, in characters.
pub const CONVERSATION_LIMIT: usize = 16_000;

fn cut(text: &str, limit: usize) -> String {
    text.chars().take(limit).collect()
}

/// The `message` and the `conversation` of a call: the prompt cut at [`MESSAGE_LIMIT`], and
/// `before` (the turns before it, oldest first) one per line as `user: …` or `assistant: …`. When
/// the lines are over [`CONVERSATION_LIMIT`] the oldest turns are dropped first, then the one that
/// is left is cut.
#[must_use]
pub fn input(prompt: &str, before: &[Turn]) -> (String, String) {
    let mut lines: VecDeque<String> = before
        .iter()
        .map(|t| {
            let role = match t.role {
                Role::User => "user",
                Role::Assistant => "assistant",
            };
            format!("{role}: {}", t.text)
        })
        .collect();
    let size = |lines: &VecDeque<String>| {
        lines.iter().map(|l| l.chars().count()).sum::<usize>() + lines.len().saturating_sub(1)
    };
    while lines.len() > 1 && size(&lines) > CONVERSATION_LIMIT {
        lines.pop_front();
    }
    if let Some(only) = lines.front_mut() {
        *only = cut(only, CONVERSATION_LIMIT);
    }
    let conversation: Vec<String> = lines.into();
    (cut(prompt, MESSAGE_LIMIT), conversation.join("\n"))
}

#[cfg(test)]
// A failed `unwrap` in a test is a failed test.
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use connectors::Author;

    fn turn(role: Role, text: &str) -> Turn {
        Turn {
            tool: "codex".into(),
            session: "s".into(),
            time: "2026-11-11T10:00:00Z".into(),
            folder: String::new(),
            branch: String::new(),
            role,
            author: Author::Unknown,
            text: text.into(),
            tools: Vec::new(),
        }
    }

    #[test]
    fn the_input_is_the_prompt_and_the_turns_before_it_one_per_line() {
        let before = [turn(Role::User, "hi"), turn(Role::Assistant, "hello")];
        let (message, conversation) = input("I write Rust", &before);
        assert_eq!(message, "I write Rust");
        assert_eq!(conversation, "user: hi\nassistant: hello");
        assert_eq!(input("x", &[]).1, "");
    }

    #[test]
    fn a_long_prompt_is_cut_at_the_message_limit_in_characters() {
        let (message, _) = input(&"é".repeat(MESSAGE_LIMIT + 5), &[]);
        assert_eq!(message.chars().count(), MESSAGE_LIMIT);
    }

    #[test]
    fn the_oldest_turns_go_first_when_the_context_is_too_long() {
        let big = "a".repeat(CONVERSATION_LIMIT / 2 - 10);
        let before = [
            turn(Role::User, "oldest"),
            turn(Role::Assistant, &big),
            turn(Role::User, &big),
        ];
        let (_, conversation) = input("p", &before);
        assert!(conversation.chars().count() <= CONVERSATION_LIMIT);
        assert!(!conversation.contains("oldest"));
        assert!(conversation.starts_with("assistant: "));
        assert!(conversation.contains("\nuser: "));
    }

    #[test]
    fn one_turn_over_the_limit_is_cut() {
        let before = [turn(Role::Assistant, &"b".repeat(CONVERSATION_LIMIT * 2))];
        let (_, conversation) = input("p", &before);
        assert_eq!(conversation.chars().count(), CONVERSATION_LIMIT);
        assert!(conversation.starts_with("assistant: bbb"));
    }
}
