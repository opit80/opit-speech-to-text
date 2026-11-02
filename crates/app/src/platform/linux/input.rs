//! Input backend choice: never repeat a possibly delivered shortcut.

#[derive(Clone, Copy)]
pub(super) enum Attempt {
    Sent,
    Unavailable,
    Uncertain,
}

pub(super) fn with_fallback(attempt: Attempt, fallback: impl FnOnce() -> bool) -> bool {
    match attempt {
        Attempt::Sent => true,
        Attempt::Unavailable => fallback(),
        Attempt::Uncertain => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fallback_runs_only_when_portal_sent_nothing() {
        for (attempt, expected, calls) in
            [(Attempt::Sent, true, 0), (Attempt::Unavailable, true, 1), (Attempt::Uncertain, false, 0)]
        {
            let mut count = 0;
            assert_eq!(
                with_fallback(attempt, || {
                    count += 1;
                    true
                }),
                expected
            );
            assert_eq!(count, calls);
        }
        assert!(!with_fallback(Attempt::Unavailable, || false));
    }
}
