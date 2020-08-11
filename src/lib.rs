use http::{HeaderMap, Method};
use warp::filters::path::FullPath;
use warp::filters::BoxedFilter;
use warp::hyper::body::Bytes;
use warp::{Filter, Rejection};

type Request = (FullPath, Method, HeaderMap, Bytes);

/// Reverse proxy filter: It forwards the request to the desired location. It maps one to one, meaning
/// that a request to `https://www.bar.foo/handle/this/path` forwarding to `https://www.other.location`
/// will result in a request to `https://www.other.location/handle/this/path`.
pub fn reverse_proxy_filter(
    root: BoxedFilter<()>,
    base_path: String,
    proxy_address: String,
) -> impl Filter<Extract = (http::Response<Bytes>,), Error = Rejection> + Clone {
    let proxy_address = warp::any().map(move || proxy_address.clone());
    let base_path = warp::any().map(move || base_path.clone());
    let data_filter = extract_request_data_filter();
    root.and(
        proxy_address
            .and(base_path)
            .and(data_filter)
            .and_then(proxy_to_and_forward_response),
    )
    .boxed()
}

/// Warp filter that extracts the relative request path, method, headers map and body of a request.
pub fn extract_request_data_filter(
) -> impl Filter<Extract = Request, Error = warp::Rejection> + Clone {
    warp::path::full()
        .and(warp::method())
        .and(warp::header::headers_cloned())
        .and(warp::body::bytes())
}

/// Build a request and send to the requested address. wraps the response into a
/// warp::reply compatible type (`http::Response`)
async fn proxy_to_and_forward_response(
    proxy_address: String,
    mut base_path: String,
    uri: FullPath,
    method: Method,
    headers: HeaderMap,
    body: Bytes,
) -> Result<http::Response<Bytes>, Rejection> {
    if !base_path.starts_with('/') {
        base_path = format!("/{}", base_path);
    }
    let request = filtered_data_to_request(proxy_address, base_path, (uri, method, headers, body));
    let response = proxy_request(request).await;
    Ok(response_to_reply(response).await)
}

/// Converts a reqwest response into a http:Response
async fn response_to_reply(response: reqwest::Response) -> http::Response<Bytes> {
    let mut builder = http::Response::builder();
    for (k, v) in response.headers() {
        builder = builder.header(k, v);
    }
    builder
        .status(response.status())
        .body(response.bytes().await.unwrap())
        .unwrap()
}

fn filtered_data_to_request(
    proxy_address: String,
    base_path: String,
    request: Request,
) -> reqwest::Request {
    let (uri, method, headers, body) = request;
    let relative_path = uri.as_str().trim_start_matches(&base_path);
    let client = reqwest::Client::new();
    let proxy_uri = format!("{}{}", proxy_address, relative_path);
    println!("{}", &proxy_uri);
    client
        .request(method, &proxy_uri)
        .headers(headers)
        .body(body)
        .build()
        .unwrap()
}

/// Build and send a request to the specified address and request data
async fn proxy_request(request: reqwest::Request) -> reqwest::Response {
    let client = reqwest::Client::new();
    client.execute(request).await.unwrap()
}

#[cfg(test)]
pub mod test {
    use crate::{
        extract_request_data_filter, filtered_data_to_request, proxy_request, reverse_proxy_filter,
        Request,
    };
    use std::net::SocketAddr;
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

        let (path, method, body, header) = ("/foo/bar", "POST", b"foo bar", ("foo", "bar"));

        let result = warp::test::request()
            .path(path)
            .method(method)
            .body(body)
            .header(header.0, header.1)
            .filter(&filter)
            .await;

        let (result_path, result_method, result_headers, result_body): Request = result.unwrap();

        assert_eq!(path, result_path.as_str());
        assert_eq!(method, result_method.as_str());
        assert_eq!(bytes::Bytes::from(body.to_vec()), result_body);
        assert_eq!(result_headers.get(header.0).unwrap(), header.1);
    }

    #[tokio::test]
    async fn proxy_forward_response() {
        let filter = extract_request_data_filter();
        let (path, method, body, header) = (
            "http://127.0.0.1:3030/foo/bar",
            "GET",
            b"foo bar",
            ("foo", "bar"),
        );

        let result = warp::test::request()
            .path(path)
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
        let request =
            filtered_data_to_request("http://127.0.0.1:4040".to_string(), "".to_string(), request);
        let response = proxy_request(request).await;
        assert_eq!(response.status(), http::status::StatusCode::OK);
    }

    #[tokio::test]
    async fn full_reverse_proxy_filter_forward_response() {
        let address_str = "http://127.0.0.1:3030";
        let filter = reverse_proxy_filter(
            warp::path!("relative_path" / ..).boxed(),
            "relative_path".to_string(),
            address_str.to_string(),
        );
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

        assert_eq!(response.status(), http::status::StatusCode::OK);
    }
}
