//! HTTP with timeouts, a size limit and no personal details in the headers.

use crate::ServiceError;
use serde_json::Value;
use std::sync::OnceLock;
use std::time::Duration;

const MAX_BODY: u64 = 32 * 1024 * 1024;

pub fn agent() -> &'static ureq::Agent {
    static AGENT: OnceLock<ureq::Agent> = OnceLock::new();
    AGENT.get_or_init(|| {
        ureq::Agent::config_builder()
            .timeout_global(Some(Duration::from_secs(30)))
            .timeout_connect(Some(Duration::from_secs(10)))
            .http_status_as_error(false)
            .user_agent(concat!("MusicCraft/", env!("CARGO_PKG_VERSION")))
            .build()
            .into()
    })
}

pub type Response = ureq::http::Response<ureq::Body>;

pub fn network(e: ureq::Error) -> ServiceError {
    // The address may hold a token, so only the kind of failure is reported.
    ServiceError::Network(match e {
        ureq::Error::HostNotFound => "the server was not found".to_owned(),
        ureq::Error::Timeout(_) => "no answer in time".to_owned(),
        ureq::Error::ConnectionFailed => "the connection failed".to_owned(),
        ureq::Error::Io(e) => e.to_string(),
        ureq::Error::Rustls(e) => e.to_string(),
        _ => "the request failed".to_owned(),
    })
}

/// The body of a successful answer, or the reason it is not one.
pub fn bytes(mut response: Response) -> Result<Vec<u8>, ServiceError> {
    let status = response.status().as_u16();
    if status == 429 {
        let wait = response.headers().get("retry-after").and_then(|v| v.to_str().ok()).and_then(|v| v.parse().ok());
        return Err(ServiceError::Wait(wait.unwrap_or(30)));
    }
    let body = response.body_mut().with_config().limit(MAX_BODY).read_to_vec().map_err(network)?;
    if (200..300).contains(&status) {
        return Ok(body);
    }
    if status == 401 {
        return Err(ServiceError::SignIn);
    }
    let json: Value = serde_json::from_slice(&body).unwrap_or(Value::Null);
    let message = ["/error/message", "/error_description", "/error"]
        .iter()
        .find_map(|p| json.pointer(p).and_then(Value::as_str))
        .unwrap_or("no reason given");
    Err(ServiceError::Refused { status, message: message.chars().take(200).collect() })
}

pub fn json(response: Response) -> Result<Value, ServiceError> {
    let body = bytes(response)?;
    if body.is_empty() {
        return Ok(Value::Null);
    }
    serde_json::from_slice(&body).map_err(|e| ServiceError::Answer(e.to_string()))
}

pub fn get(url: &str) -> Result<Response, ServiceError> {
    agent().get(url).call().map_err(network)
}

/// A text field of a JSON object.
pub fn text(v: &Value, key: &str) -> String {
    match v.get(key) {
        Some(Value::String(s)) => s.clone(),
        Some(Value::Number(n)) => n.to_string(),
        _ => String::new(),
    }
}

/// A whole-number field of a JSON object.
pub fn number(v: &Value, key: &str) -> u64 {
    v.get(key).and_then(Value::as_u64).unwrap_or(0)
}
