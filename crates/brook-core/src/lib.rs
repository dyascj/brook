//! The engine behind Brook, a calm, local RSS reader.
//!
//! This crate has no UI. It keeps subscriptions and articles in SQLite
//! ([`store`]), fetches and parses feeds ([`http`], [`parse`]), finds feeds
//! for a website ([`discover`]), refreshes them ([`refresh`]), reads and
//! writes OPML ([`opml`]), pulls full articles from web pages ([`reader`]),
//! and turns article HTML into a document the UI can typeset ([`document`]).
//!
//! Everything here is synchronous. The app runs network work on background
//! threads; each thread opens its own [`Store`].

#![warn(missing_docs)]

pub mod cache;
pub mod catalog;
pub mod discover;
pub mod document;
pub mod error;
pub mod http;
pub mod model;
pub mod opml;
pub mod parse;
pub mod paths;
pub mod reader;
pub mod refresh;
pub mod settings;
pub mod store;
pub mod text;
pub mod time;
pub mod video;

#[cfg(test)]
mod testing;

pub use error::{Error, Result};
pub use store::Store;
