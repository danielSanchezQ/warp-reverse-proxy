use thiserror::Error;
use warp::reject::Reject;

/// Lib errors wrapper
/// Encapsulates the different errors that can occur during forwarding requests
#[derive(Error, Debug)]
pub enum Error {
    /// Errors produced by reading or building requests
    #[error("Request error: {0}")]
    Request(#[from] reqwest::Error),

    // FIXME: allow warning for now, must be renamed for next breaking api version
    #[allow(clippy::upper_case_acronyms)]
    /// Errors when connecting to the target service
    #[error("Http error: {0}")]
    HTTP(#[from] warp::http::Error),
}

impl Reject for Error {}
