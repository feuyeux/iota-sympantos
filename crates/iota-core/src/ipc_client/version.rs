//! Pure protocol-version negotiation, shared by every desktop IPC server and
//! client. Extracted from `iota-core::daemon::desktop::negotiate_version` so
//! non-daemon IPC servers (e.g. cockpit-runner) can reuse the same min/max
//! negotiation rule without depending on the daemon's wire message enum.

use std::fmt;

/// The client's declared version range failed to overlap with the server's
/// supported range.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VersionNegotiationError {
    pub client_min: u32,
    pub client_max: u32,
    pub server_min: u32,
    pub server_max: u32,
}

impl fmt::Display for VersionNegotiationError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "Protocol version mismatch: client [{},{}] vs server [{},{}]",
            self.client_min, self.client_max, self.server_min, self.server_max
        )
    }
}

impl std::error::Error for VersionNegotiationError {}

/// Negotiate a protocol version given a client's declared `[client_min,
/// client_max]` range and a server's supported `[server_min, server_max]`
/// range.
///
/// The negotiated version is the highest version both sides can speak: the
/// smaller of the two `max` values. This is only valid if it still falls
/// within both sides' `min` bound; otherwise the ranges do not overlap and
/// negotiation fails.
///
/// This mirrors a client that only sends a single `protocol_version` (no
/// explicit range) by having the caller pass `client_min == client_max ==
/// protocol_version`, preserving backward compatibility with older clients.
pub fn negotiate_version(
    client_min: u32,
    client_max: u32,
    server_min: u32,
    server_max: u32,
) -> Result<u32, VersionNegotiationError> {
    let negotiated = client_max.min(server_max);
    if negotiated < server_min || negotiated < client_min {
        return Err(VersionNegotiationError {
            client_min,
            client_max,
            server_min,
            server_max,
        });
    }
    Ok(negotiated)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn negotiates_the_highest_mutually_supported_version() {
        assert_eq!(negotiate_version(2, 3, 2, 3), Ok(3));
    }

    #[test]
    fn backward_compatible_client_without_a_range_still_negotiates() {
        // A v2-only client sends protocol_version=2 with no explicit range,
        // i.e. client_min == client_max == 2.
        assert_eq!(negotiate_version(2, 2, 2, 3), Ok(2));
    }

    #[test]
    fn rejects_a_client_range_entirely_below_the_server_minimum() {
        let err = negotiate_version(0, 1, 2, 3).unwrap_err();
        assert_eq!(
            err,
            VersionNegotiationError {
                client_min: 0,
                client_max: 1,
                server_min: 2,
                server_max: 3,
            }
        );
    }

    #[test]
    fn rejects_a_client_range_entirely_above_the_server_maximum() {
        assert!(negotiate_version(5, 6, 2, 3).is_err());
    }

    #[test]
    fn negotiated_version_is_capped_by_the_narrower_of_the_two_maximums() {
        assert_eq!(negotiate_version(2, 10, 2, 3), Ok(3));
    }
}
