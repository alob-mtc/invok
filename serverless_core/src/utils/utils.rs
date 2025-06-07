use axum::body::Body;
use axum::http::{
    HeaderMap, Request as AxumRequest, Response as AxumResponse, StatusCode as AxumStatusCode,
    StatusCode,
};
use axum::response::IntoResponse;
use hyper::body::to_bytes;
use rand::distributions::Alphanumeric;
use rand::{thread_rng, Rng};
use reqwest::header::HeaderMap as ReqwestHeaderMap;
use reqwest::Client;
use reqwest::StatusCode as ReqwestStatusCode;
use std::collections::HashMap;
use std::fs;
use std::fs::File;
use std::path::PathBuf;
use std::time::Duration;
use tracing::{debug, error, warn};
use urlencoding::encode;
use uuid::Uuid;

/// A RAII guard that runs a closure when dropped.
///
/// This is useful for deferring code until the scope exits.
pub struct ScopeCall<F: FnMut()> {
    pub c: Option<F>,
}

impl<F: FnMut()> Drop for ScopeCall<F> {
    fn drop(&mut self) {
        // If the closure exists, call it. Otherwise log a warning.
        if let Some(mut callback) = self.c.take() {
            callback();
        } else {
            warn!("ScopeCall callback was already taken or missing on drop.");
        }
    }
}

/// Returns a `ScopeCall` that will execute the provided closure when dropped.
///
/// # Examples
/// ```
/// let _deferred = defer_fn(|| println!("This will run when _deferred goes out of scope"));
/// ```
pub fn defer_fn<T: FnMut()>(c: T) -> ScopeCall<T> {
    ScopeCall { c: Some(c) }
}

/// Converts a map of environment variables into a string in the format:
/// `ENV key="value"\n` for each variable.
pub fn envs_to_string(envs: HashMap<String, String>) -> String {
    let mut envs_str = String::new();
    for (key, value) in envs {
        envs_str.push_str(&format!("ENV {}=\"{}\"\n", key, value));
    }
    envs_str
}

/// Converts a reqwest status code into an Axum status code.
/// Falls back to `INTERNAL_SERVER_ERROR` if the conversion fails.
fn convert_status_code(reqwest_status: ReqwestStatusCode) -> AxumStatusCode {
    AxumStatusCode::from_u16(reqwest_status.as_u16())
        .unwrap_or(AxumStatusCode::INTERNAL_SERVER_ERROR)
}

/// Converts Axum headers into reqwest headers.
fn convert_axum_headers_to_req_header(headers: HeaderMap) -> ReqwestHeaderMap {
    let mut header_res = ReqwestHeaderMap::new();
    for (hn, hv) in headers.iter() {
        header_res.append(hn, hv.clone());
    }
    header_res
}

/// Converts reqwest headers into Axum headers.
///
/// Removes the `TRANSFER_ENCODING` header from the source before copying.
fn convert_req_header_to_axum_headers(
    req_headers: &mut ReqwestHeaderMap,
    res_headers: &mut HeaderMap,
) {
    req_headers.remove(http::header::TRANSFER_ENCODING);

    for (hn, hv) in req_headers.iter() {
        debug!("Converting header - {}: {:?}", hn, hv.to_str());
        res_headers.append(hn, hv.clone());
    }
}

/// Creates a URL from the given address, key, and query parameters.
///
/// The query parameters are URL-encoded.
///
/// # Arguments
///
/// * `addr` - The host address (and port) of the target service.
/// * `key` - The endpoint or function key to call.
/// * `query` - A map of query parameters.
///
/// # Returns
///
/// A complete URL as a string.
fn create_url(addr: &str, key: &str, query: HashMap<String, String>) -> String {
    let mut url = format!("http://{}/{}", addr, key);

    if !query.is_empty() {
        let query_string = query
            .iter()
            .map(|(k, v)| format!("{}={}", encode(k), encode(v))) // Escape query parameters
            .collect::<Vec<_>>()
            .join("&");

        url.push('?');
        url.push_str(&query_string);
    }

    url
}

/// Forwards an incoming Axum request to a downstream service.
///
/// This function builds an HTTP request to the given service address and key,
/// forwarding the method, headers, and body of the original request.
///
/// It supports GET and POST methods. For other methods, a `METHOD_NOT_ALLOWED`
/// response is returned.
///
/// # Arguments
///
/// * `addr` - The downstream service address.
/// * `key` - The function key to call on the downstream service.
/// * `query` - Query parameters to include in the request URL.
/// * `headers` - The headers from the original request.
/// * `req` - The original Axum request.
///
/// # Returns
///
/// An Axum response generated from the downstream service's response.
pub async fn make_request(
    addr: &str,
    key: &str,
    query: HashMap<String, String>,
    headers: HeaderMap,
    req: AxumRequest<Body>,
) -> impl IntoResponse {
    let client = Client::builder()
        .timeout(Duration::from_secs(20))
        .build()
        .expect("Failed to build HTTP client");

    // Choose the appropriate client method based on the request method.
    let response_result = match *req.method() {
        http::Method::GET => {
            client
                .get(create_url(addr, key, query))
                .headers(convert_axum_headers_to_req_header(headers))
                .send()
                .await
        }
        http::Method::POST => {
            let body_bytes = match to_bytes(req.into_body()).await {
                Ok(bytes) => bytes,
                Err(err) => {
                    error!("Error reading request body: {:?}", err);
                    return AxumResponse::builder()
                        .status(StatusCode::BAD_REQUEST)
                        .body("Could not read request body".to_owned())
                        .unwrap();
                }
            };

            client
                .post(create_url(addr, key, query))
                .headers(convert_axum_headers_to_req_header(headers))
                .body(body_bytes)
                .send()
                .await
        }
        _ => {
            return AxumResponse::builder()
                .status(StatusCode::METHOD_NOT_ALLOWED)
                .body(format!(
                    "We don't currently support {} functions",
                    req.method()
                ))
                .unwrap();
        }
    };

    // Process the downstream service response.
    let response = match response_result {
        Ok(res) => {
            let status = convert_status_code(res.status());
            let mut downstream_headers = res.headers().clone();

            // Attempt to read the response text.
            match res.text().await {
                Ok(text) => {
                    let mut response = AxumResponse::builder().status(status).body(text).unwrap();
                    let headers_mut = response.headers_mut();
                    convert_req_header_to_axum_headers(&mut downstream_headers, headers_mut);
                    response
                }
                Err(err) => {
                    error!("Failed to read downstream response: {:?}", err);
                    AxumResponse::builder()
                        .status(StatusCode::INTERNAL_SERVER_ERROR)
                        .body("Failed to read downstream response".to_owned())
                        .unwrap()
                }
            }
        }
        Err(e) => {
            error!("Error making downstream request: {:?}", e);
            AxumResponse::builder()
                .status(StatusCode::INTERNAL_SERVER_ERROR)
                .body("Failed to make downstream request".to_string())
                .unwrap()
        }
    };

    response
}

/// Creates a base file structure for a function.
///
/// If the specified path already exists, an error is returned. Otherwise, the
/// directory is created and a `main.go` file is initialized in that directory.
///
/// # Arguments
///
/// * `path` - The directory path where the function files will be created.
/// * `name` - The name of the function (used in error messages).
/// * `_runtime` - The runtime (currently unused, but reserved for future use).
///
/// # Returns
///
/// A `Result` containing the created `File` handle for `main.go` or an `std::io::Error`.
pub fn create_fn_files_base(path: &PathBuf, name: &str, _runtime: &str) -> std::io::Result<File> {
    if path.exists() {
        return Err(std::io::Error::new(
            std::io::ErrorKind::AlreadyExists,
            format!("Folder '{}' already exists.", name),
        ));
    }

    fs::create_dir(path)?;

    let main_file_path = path.join("main.go");
    let main_file = File::create(&main_file_path)?;

    Ok(main_file)
}

pub fn generate_hash(source: Uuid) -> String {
    let uuid_hash = format!("{:x}", md5::compute(source.to_string()));
    let uuid_short = &uuid_hash[0..20]; // Use first 20 characters

    uuid_short.to_string()
}
