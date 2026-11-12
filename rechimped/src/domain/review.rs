/// Whether an entry's notes say anything: missing, empty and whitespace-only notes don't.
pub fn has_notes(notes: Option<&str>) -> bool {
    notes.is_some_and(|n| !n.trim().is_empty())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn notes_need_more_than_whitespace() {
        assert!(has_notes(Some("Refactor login")));
        assert!(has_notes(Some("  x ")));
        assert!(!has_notes(None));
        assert!(!has_notes(Some("")));
        assert!(!has_notes(Some(" \t\n ")));
    }
}
