use anyhow::{Result, bail};

use super::types::Status;

/// Returns `Ok(())` when the transition `from -> to` is permitted, or an
/// error describing why it is not.
pub fn validate_transition(from: Status, to: Status) -> Result<()> {
    let valid = matches!(
        (from, to),
        (Status::Triage, Status::Todo)
            | (Status::Todo, Status::Ready)
            | (Status::Ready, Status::Running)
            | (Status::Running, Status::Done)
            | (Status::Running, Status::Blocked)
            | (Status::Running, Status::Ready)   // claim expired
            | (Status::Blocked, Status::Ready)
            | (Status::Blocked, Status::Done)    // abandoned / manually resolved
            | (Status::Done, Status::Archived)
    );

    if valid {
        Ok(())
    } else {
        bail!("invalid status transition: {} -> {}", from, to)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn valid_transitions() {
        assert!(validate_transition(Status::Triage, Status::Todo).is_ok());
        assert!(validate_transition(Status::Todo, Status::Ready).is_ok());
        assert!(validate_transition(Status::Ready, Status::Running).is_ok());
        assert!(validate_transition(Status::Running, Status::Done).is_ok());
        assert!(validate_transition(Status::Running, Status::Blocked).is_ok());
        assert!(validate_transition(Status::Running, Status::Ready).is_ok());
        assert!(validate_transition(Status::Blocked, Status::Ready).is_ok());
        assert!(validate_transition(Status::Done, Status::Archived).is_ok());
    }

    #[test]
    fn invalid_transitions() {
        assert!(validate_transition(Status::Triage, Status::Running).is_err());
        assert!(validate_transition(Status::Todo, Status::Done).is_err());
        assert!(validate_transition(Status::Ready, Status::Archived).is_err());
        assert!(validate_transition(Status::Done, Status::Todo).is_err());
        assert!(validate_transition(Status::Archived, Status::Triage).is_err());
    }

    #[test]
    fn blocked_to_done_is_valid() {
        assert!(validate_transition(Status::Blocked, Status::Done).is_ok());
    }
}
