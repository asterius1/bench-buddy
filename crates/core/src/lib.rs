//! Bench Buddy's shared pieces, used by every build crate, the checker and the addon: the raw
//! arcdps log reader, one player's record of a pull, the helpers the builds share (the bench's casts,
//! matching yours with them, number formats) and the downloads.

pub mod build;
pub mod difflib;
pub mod evtc;
pub mod kit;
pub mod live;
pub mod omap;
pub mod rot;
pub mod web;
