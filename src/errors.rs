use thiserror::Error;
use warp::reject::Reject;

#[derive(Error, Debug)]
pub enum Error {
    #[error("Request error: {0}")]
    Request(#[from] reqwest::Error),

    #[error("Http error: {0}")]
    HTTP(#[from] http::Error),
}

impl Reject for Error {}
