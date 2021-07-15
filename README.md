# warp-reverse-proxy

[![MIT licensed](https://img.shields.io/badge/license-MIT-blue.svg)](./LICENSE)
[![GHA Build Status](https://github.com/danielsanchezq/warp-reverse-proxy/workflows/CI/badge.svg)](https://github.com/danielsanchezq/warp-reverse-proxy/actions?query=workflow%3ACI)
[![Docs Badge](https://docs.rs/warp-reverse-proxy/badge.svg)](https://docs.rs/warp-reverse-proxy)

Fully composable [warp](https://github.com/seanmonstar/warp) filter that can be used as a reverse proxy. It forwards the request to the 
desired address and replies back the remote address response.

### Add the library dependency
```toml
[dependencies]
warp = "0.3"
warp-reverse-proxy = "0.3"
```

### Use it as simple as:
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


### For more control. You can compose inner library filters to help you compose your own reverse proxy:

```rust
#[tokio::main]
async fn main() {
    let hello = warp::path!("hello" / String).map(|name| format!("Hello port, {}!", name));

    // // spawn base server
    tokio::spawn(warp::serve(hello).run(([0, 0, 0, 0], 8080)));

    let request_filter = extract_request_data_filter();
    let app = warp::path!("hello" / String)
        // build proxy address and base path data from current filter
        .map(|port| (format!("http://127.0.0.1:{}/", port), "".to_string()))
        .untuple_one()
        // build the request with data from previous filters
        .and(request_filter)
        .and_then(proxy_to_and_forward_response)
        .and_then(log_response);

    // spawn proxy server
    warp::serve(app).run(([0, 0, 0, 0], 3030)).await;
}
```

### Requests client initialization

By default, a simple `reqwests::Client` is initialized and used.
In case some specific client configuration need to be used it can be overridden:

```rust
use warp_reverse_proxy::{reverse_proxy_filter, CLIENT as PROXY_CLIENT};

#[tokio::main]
async fn main() {
    let client = reqwest::Client::builder()
        .default_headers(headers)
        .build().expect("client goes boom...");
    PROXY_CLIENT.set(client).expect("client couldn't be set");
    ...
}
```