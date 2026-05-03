//! App-facing extension point.
//!
//! This module is reserved for application/read-model state that should not be
//! coupled to terminal command parsing or ACP process management.

#[allow(dead_code)]
pub struct AppRuntime;

#[allow(dead_code)]
impl AppRuntime {
    pub fn new() -> Self {
        Self
    }
}
