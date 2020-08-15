# warp-reverse-proxy

[![MIT licensed](https://img.shields.io/badge/license-MIT-blue.svg)](./LICENSE)
[![GHA Build Status](https://github.com/danielsanchezq/warp-reverse-proxy/workflows/CI/badge.svg)](https://github.com/danielsanchezq/warp-reverse-proxy/actions?query=workflow%3ACI)

Fully composable [warp](https://github.com/seanmonstar/warp) filter that can be used as a reverse proxy. It forwards the request to the 
desired address and replies back the remote address response.


```rust
use warp::{hyper::body::Bytes, Filter, Rejection, Reply};
use warp_reverse_proxy::reverse_proxy_filter;

async fn log_response(response: http::Response<Bytes>) -> Result<impl Reply, Rejection> {
    println!("{:?}", response);
    Ok(response)
}

#[tokio::main]
async fn main() {
    let hello = warp::path!("hello" / String).map(|name| format!("Hello, {}!", name));

    // // spawn base server
    tokio::spawn(warp::serve(hello).run(([0, 0, 0, 0], 8080)));

    // Forward request to localhost in other port
    let app = warp::path!("hello" / ..).and(
        reverse_proxy_filter("".to_string(), "http://127.0.0.1:8080/".to_string())
            .and_then(log_response),
    );

    // spawn proxy server
    warp::serve(app).run(([0, 0, 0, 0], 3030)).await;
}
```