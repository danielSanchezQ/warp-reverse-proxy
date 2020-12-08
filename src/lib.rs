//! Fully composable [warp](https://github.com/seanmonstar/warp) filter that can be used as a reverse proxy. It forwards the request to the
//! desired address and replies back the remote address response.
//!
//!
//! ```no_run
//! use warp::{hyper::body::Bytes, Filter, Rejection, Reply, http::Response};
//! use warp_reverse_proxy::reverse_proxy_filter;
//!
//! async fn log_response(response: Response<Bytes>) -> Result<impl Reply, Rejection> {
//!     println!("{:?}", response);
//!     Ok(response)
//! }
//!
//! #[tokio::main]
//! async fn main() {
//!     let hello = warp::path!("hello" / String).map(|name| format!("Hello, {}!", name));
//!
//!     // // spawn base server
//!     tokio::spawn(warp::serve(hello).run(([0, 0, 0, 0], 8080)));
//!
//!     // Forward request to localhost in other port
//!     let app = warp::path!("hello" / ..).and(
//!         reverse_proxy_filter("".to_string(), "http://127.0.0.1:8080/".to_string())
//!             .and_then(log_response),
//!     );
//!
//!     // spawn proxy server
//!     warp::serve(app).run(([0, 0, 0, 0], 3030)).await;
//! }
//! ```
mod errors;

use lazy_static::lazy_static;
use unicase::Ascii;
use warp::filters::path::FullPath;
use warp::http;
use warp::http::{HeaderMap, HeaderValue, Method};
use warp::hyper::body::Bytes;
use warp::{Filter, Rejection};

/// Wrapper around query parameters.
///
/// This is the type that holds the request query parameters.
pub type QueryParameters = Option<String>;

/// Wrapper around a request data.
///
/// It is the type that holds the request data extracted by the [`extract_request_data_filter`](fn.extract_request_data_filter.html) filter.
pub type Request = (FullPath, QueryParameters, Method, HeaderMap, Bytes);

/// Reverse proxy filter:
/// Forwards the request to the desired location. It maps one to one, meaning
/// that a request to `https://www.bar.foo/handle/this/path` forwarding to `https://www.other.location`
/// will result in a request to `https://www.other.location/handle/this/path`.
///
/// # Arguments
///
/// * `base_path` - A string with the initial relative path of the endpoint.
/// For example a `foo/` applied for an endpoint `foo/bar/` will result on a proxy to `bar/` (hence `/foo` is removed)
///
/// * `proxy_address` - Base proxy address to forward request.
/// # Examples
///
/// When making a filter with a path `/handle/this/path` combined with a filter built
/// with `reverse_proxy_filter("handle".to_string(), "localhost:8080")`
/// will make that request arriving to `https://www.bar.foo/handle/this/path` be forwarded to `localhost:8080/this/path`
pub fn reverse_proxy_filter(
    base_path: String,
    proxy_address: String,
) -> impl Filter<Extract = (http::Response<Bytes>,), Error = Rejection> + Clone {
    let proxy_address = warp::any().map(move || proxy_address.clone());
    let base_path = warp::any().map(move || base_path.clone());
    let data_filter = extract_request_data_filter();

    proxy_address
        .and(base_path)
        .and(data_filter)
        .and_then(proxy_to_and_forward_response)
        .boxed()
}

/// Warp filter that extracts query parameters from the request, if they exist.
pub fn query_params_filter(
) -> impl Filter<Extract = (QueryParameters,), Error = std::convert::Infallible> + Clone {
    warp::query::raw()
        .map(Some)
        .or_else(|_| async { Ok::<(QueryParameters,), std::convert::Infallible>((None,)) })
}

/// Warp filter that extracts the relative request path, method, headers map and body of a request.
pub fn extract_request_data_filter(
) -> impl Filter<Extract = Request, Error = warp::Rejection> + Clone {
    warp::path::full()
        .and(query_params_filter())
        .and(warp::method())
        .and(warp::header::headers_cloned())
        .and(warp::body::bytes())
}

/// Build a request and send to the requested address. wraps the response into a
/// warp::reply compatible type (`http::Response`)
///
/// # Arguments
///
/// * `proxy_address` - A string containing the base proxy address where the request
/// will be forwarded to.
///
/// * `base_path` - A string with the prepended sub-path to be stripped from the request uri path.
///
/// * `uri` -> The uri of the extracted request.
///
/// * `method` -> The request method.
///
/// * `headers` -> The request headers.
///
/// * `body` -> The request body.
///
/// # Examples
/// Notice that this method usually need to be used in aggregation with
/// the [`extract_request_data_filter`](fn.extract_request_data_filter.html) filter` which already
/// provides the `(uri, method, headers, body)` needed for calling this method. But the `proxy_address`
/// and the `base_path` arguments need to be provided too.
/// ```rust, ignore
/// let request_filter = extract_request_data_filter();
///     let app = warp::path!("hello" / String)
///         .map(|port| (format!("http://127.0.0.1:{}/", port), "".to_string()))
///         .untuple_one()
///         .and(request_filter)
///         .and_then(proxy_to_and_forward_response)
///         .and_then(log_response);
/// ```
pub async fn proxy_to_and_forward_response(
    proxy_address: String,
    base_path: String,
    uri: FullPath,
    params: QueryParameters,
    method: Method,
    headers: HeaderMap,
    body: Bytes,
) -> Result<http::Response<Bytes>, Rejection> {
    let proxy_uri = remove_relative_path(&uri, base_path, proxy_address);
    let request = filtered_data_to_request(proxy_uri, (uri, params, method, headers, body))
        .map_err(warp::reject::custom)?;
    let response = proxy_request(request).await.map_err(warp::reject::custom)?;
    response_to_reply(response)
        .await
        .map_err(warp::reject::custom)
}

/// Converts a reqwest response into a http:Response
async fn response_to_reply(
    response: reqwest::Response,
) -> Result<http::Response<Bytes>, errors::Error> {
    let mut builder = http::Response::builder();
    for (k, v) in remove_hop_headers(response.headers()).iter() {
        builder = builder.header(k, v);
    }
    builder
        .status(response.status())
        .body(response.bytes().await.map_err(errors::Error::Request)?)
        .map_err(errors::Error::HTTP)
}

fn remove_relative_path(uri: &FullPath, base_path: String, proxy_address: String) -> String {
    let mut base_path = base_path;
    if !base_path.starts_with('/') {
        base_path = format!("/{}", base_path);
    }
    let relative_path = uri
        .as_str()
        .trim_start_matches(&base_path)
        .trim_start_matches('/');

    let proxy_address = proxy_address.trim_end_matches('/');
    format!("{}/{}", proxy_address, relative_path)
}

/// Checker method to filter hop headers
/// Headers are checked using unicase to avoid case misfunctions
fn is_hop_header(header_name: &str) -> bool {
    lazy_static! {
        static ref HOP_HEADERS: Vec<Ascii<&'static str>> = vec![
            Ascii::new("Connection"),
            Ascii::new("Keep-Alive"),
            Ascii::new("Proxy-Authenticate"),
            Ascii::new("Proxy-Authorization"),
            Ascii::new("Te"),
            Ascii::new("Trailers"),
            Ascii::new("Transfer-Encoding"),
            Ascii::new("Upgrade"),
        ];
    }

    HOP_HEADERS.iter().any(|h| h == &header_name)
}

fn remove_hop_headers(headers: &HeaderMap<HeaderValue>) -> HeaderMap<HeaderValue> {
    headers
        .iter()
        .filter_map(|(k, v)| {
            if !is_hop_header(k.as_str()) {
                Some((k.clone(), v.clone()))
            } else {
                None
            }
        })
        .collect()
}

fn filtered_data_to_request(
    proxy_address: String,
    request: Request,
) -> Result<reqwest::Request, errors::Error> {
    let (uri, params, method, headers, body) = request;

    let relative_path = uri.as_str().trim_start_matches('/');

    let proxy_address = proxy_address.trim_end_matches('/');

    let proxy_uri = if let Some(params) = params {
        format!("{}/{}?{}", proxy_address, relative_path, params)
    } else {
        format!("{}/{}", proxy_address, relative_path)
    };

    let headers = remove_hop_headers(&headers);

    let client = reqwest::Client::new();
    client
        .request(method, &proxy_uri)
        .headers(headers)
        .body(body)
        .build()
        .map_err(errors::Error::Request)
}

/// Build and send a request to the specified address and request data
async fn proxy_request(request: reqwest::Request) -> Result<reqwest::Response, errors::Error> {
    let client = reqwest::Client::new();
    client
        .execute(request)
        .await
        .map_err(errors::Error::Request)
}

#[cfg(test)]
pub mod test {
    use crate::{
        extract_request_data_filter, filtered_data_to_request, proxy_request, remove_relative_path,
        reverse_proxy_filter, Request,
    };
    use std::net::SocketAddr;
    use warp::http::StatusCode;
    use warp::Filter;

    fn serve_test_response(path: String, address: SocketAddr) {
        if path.is_empty() {
            tokio::spawn(warp::serve(warp::any().map(warp::reply)).run(address));
        } else {
            tokio::spawn(warp::serve(warp::path(path).map(warp::reply)).run(address));
        }
    }

    #[tokio::test]
    async fn request_data_match() {
        let filter = extract_request_data_filter();

        let (path, query, method, body, header) =
            ("/foo/bar", "foo=bar", "POST", b"foo bar", ("foo", "bar"));
        let path_with_query = format!("{}?{}", path, query);

        let result = warp::test::request()
            .path(path_with_query.as_str())
            .method(method)
            .body(body)
            .header(header.0, header.1)
            .filter(&filter)
            .await;

        let (result_path, result_query, result_method, result_headers, result_body): Request =
            result.unwrap();

        assert_eq!(path, result_path.as_str());
        assert_eq!(Some(query.to_string()), result_query);
        assert_eq!(method, result_method.as_str());
        assert_eq!(bytes::Bytes::from(body.to_vec()), result_body);
        assert_eq!(result_headers.get(header.0).unwrap(), header.1);
    }

    #[tokio::test]
    async fn proxy_forward_response() {
        let filter = extract_request_data_filter();
        let (path_with_params, method, body, header) = (
            "http://127.0.0.1:3030/foo/bar?foo=bar",
            "GET",
            b"foo bar",
            ("foo", "bar"),
        );

        let result = warp::test::request()
            .path(path_with_params)
            .method(method)
            .body(body)
            .header(header.0, header.1)
            .filter(&filter)
            .await;

        let request: Request = result.unwrap();

        let address = ([127, 0, 0, 1], 4040);
        serve_test_response("".to_string(), address.into());

        tokio::task::yield_now().await;
        // transform request data into an actual request
        let request = filtered_data_to_request(
            remove_relative_path(
                &request.0,
                "".to_string(),
                "http://127.0.0.1:4040".to_string(),
            ),
            request,
        )
        .unwrap();
        let response = proxy_request(request).await.unwrap();
        assert_eq!(response.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn full_reverse_proxy_filter_forward_response() {
        let address_str = "http://127.0.0.1:3030";
        let filter = warp::path!("relative_path" / ..).and(reverse_proxy_filter(
            "relative_path".to_string(),
            address_str.to_string(),
        ));
        let address = ([127, 0, 0, 1], 3030);
        let (path, method, body, header) = (
            "https://127.0.0.1:3030/relative_path/foo",
            "GET",
            b"foo bar",
            ("foo", "bar"),
        );

        serve_test_response("foo".to_string(), address.into());
        tokio::task::yield_now().await;

        let response = warp::test::request()
            .path(path)
            .method(method)
            .body(body)
            .header(header.0, header.1)
            .reply(&filter)
            .await;

        assert_eq!(response.status(), StatusCode::OK);
    }
}
