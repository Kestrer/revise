//! Parser for the `.set` files read by `revise`.
#![warn(clippy::all, clippy::pedantic)]
#![allow(clippy::missing_panics_doc, clippy::range_plus_one)]
#![warn(missing_docs)]

mod set;
pub use set::*;

mod guess;
pub use guess::*;
