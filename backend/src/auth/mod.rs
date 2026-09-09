//! Authentication: single local administrator, always on (spec §7.1).
//!
//! Two credentials reach the same account. A browser presents the session
//! cookie in [`session`] and gets everything the administrator can do; a
//! machine client presents a bearer token from [`token`] and gets only what
//! its capability allows. Neither can be turned off.

pub mod password;
pub mod ratelimit;
pub mod session;
pub mod token;
