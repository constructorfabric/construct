//! The reasons an answer fails, with the plain text the client is told. The texts are part of what
//! the windows show: they do not change.

use chat_core::Failure;

#[test]
fn every_failure_reason_has_its_text() {
    for (failure, text) in [
        (Failure::KeyRefused, "key refused"),
        (Failure::ModelNotFound, "model not found"),
        (Failure::Unreachable, "unreachable"),
        (Failure::Timeout, "timeout"),
        (Failure::Status(500), "error 500"),
        (Failure::Status(429), "error 429"),
        // The refusal of tools next to a reasoning effort is asked again; if it stays, it is a 400.
        (Failure::ToolsWithReasoning, "error 400"),
        (Failure::TooLong, "too long"),
        (Failure::Cut, "cut"),
        (Failure::CannotSave, "cannot save"),
        (Failure::Internal, "error"),
    ] {
        assert_eq!(failure.reason(), text, "{failure:?}");
    }
}
