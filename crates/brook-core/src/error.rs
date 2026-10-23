//! The crate's error type.

use std::io;

/// Everything that can go wrong inside `brook-core`.
///
/// Variants carry enough context to show a short, human message in the UI
/// (`to_string()`), while keeping the source error for logs.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// The SQLite database rejected a query or could not be opened.
    #[error("database error: {0}")]
    Database(#[from] rusqlite::Error),

    /// A network request failed before a response arrived (DNS, TLS, timeout).
    #[error("network error: {0}")]
    Network(String),

    /// The server answered with a non-success status.
    #[error("the server answered {status}")]
    Status {
        /// The HTTP status code.
        status: u16,
    },

    /// A URL could not be understood.
    #[error("not a valid web address: {0}")]
    InvalidUrl(String),

    /// The document is not an RSS, Atom, or JSON feed.
    #[error("not a feed: {0}")]
    NotAFeed(String),

    /// No feed could be found for a website.
    #[error("no feed found at {0}")]
    NoFeedFound(String),

    /// Reader mode found no article on a page.
    #[error("couldn't find an article on that page")]
    Unreadable,

    /// An OPML document could not be read.
    #[error("could not read OPML: {0}")]
    Opml(String),

    /// A row that should exist does not.
    #[error("{0} not found")]
    NotFound(&'static str),

    /// A file could not be read or written.
    #[error("file error: {0}")]
    Io(#[from] io::Error),
}

impl From<ureq::Error> for Error {
    fn from(error: ureq::Error) -> Self {
        match error {
            ureq::Error::StatusCode(status) => Error::Status { status },
            other => Error::Network(other.to_string()),
        }
    }
}

/// Shorthand for results in this crate.
pub type Result<T, E = Error> = std::result::Result<T, E>;
