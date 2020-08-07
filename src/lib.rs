use http::{HeaderMap, Method};
use warp::filters::path::FullPath;
use warp::filters::BoxedFilter;
use warp::hyper::body::Bytes;
use warp::{Filter, Rejection, Reply};

type Request = (FullPath, Method, HeaderMap, Bytes);

/// Reverse proxy filter: It forwards the request to the desired location. It maps one to one, meaning
/// that a request to `https://www.bar.foo/handle/this/path` forwarding to `https://www.other.location`
/// will result in a request to `https://www.other.location/handle/this/path`.
pub fn reverse_proxy_filter(
    root: BoxedFilter<()>,
    proxy_address: String,
) -> impl Filter<Extract = impl Reply, Error = Rejection> + Clone {
    let proxy_address = warp::any().map(move || proxy_address.clone());
    let data_filter = extract_request_data_filter();
    root.and(
        proxy_address
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
    uri: FullPath,
    method: Method,
    headers: HeaderMap,
    body: Bytes,
) -> Result<impl Reply, Rejection> {
    let response = proxy_to(proxy_address, (uri, method, headers, body)).await;
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

/// Build and send a request to the specified address and request data
async fn proxy_to(proxy_address: String, request: Request) -> reqwest::Response {
    let (uri, method, headers, body) = request;

    let client = reqwest::Client::new();
    let request = client
        .request(
            method,
            format!("{}{}", proxy_address, uri.as_str()).as_str(),
        )
        .headers(headers)
        .body(body)
        .build()
        .unwrap();
    client.execute(request).await.unwrap()
}

#[cfg(test)]
pub mod test {
    use crate::{extract_request_data_filter, proxy_to, reverse_proxy_filter, Request};
    use std::net::SocketAddr;
    use warp::Filter;

    fn serve_test_response(address: SocketAddr) {
        let app = warp::any().map(warp::reply);
        tokio::spawn(warp::serve(app).run(address));
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
        serve_test_response(address.into());

        let response = proxy_to("http://127.0.0.1:4040".to_string(), request).await;
        assert_eq!(response.status(), http::status::StatusCode::OK);
    }

    #[tokio::test]
    async fn full_reverse_proxy_filter_forward_response() {
        let address_str = "http://127.0.0.1:3030";
        let filter = reverse_proxy_filter(warp::any().boxed(), address_str.to_string());
        let address = ([127, 0, 0, 1], 3030);
        let (path, method, body, header) = (
            "https://127.0.0.1:3030/foo/bar",
            "GET",
            b"foo bar",
            ("foo", "bar"),
        );

        serve_test_response(address.into());

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
