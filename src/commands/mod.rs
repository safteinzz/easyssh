//! The thin CLI surface. By design only the handful of actions where *typing
//! beats pointing* live here - connect, list, copy. Everything you'd otherwise
//! have to look up (keys, tunnels, mounts, adding hosts) lives in the TUI, where
//! there's nothing to memorize.

pub mod connect;
pub mod cp;
pub mod ls;
pub mod selfcmd;
