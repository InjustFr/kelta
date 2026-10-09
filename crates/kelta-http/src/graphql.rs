//! GraphQL over the per-account [`Authed`] client (GitHub).

use kelta_proto::error::{ErrorCode, KeltaError};
use serde_json::{Value, json};

use crate::{Authed, HttpRequest};

/// POST `{query, variables}` to `url` and return `data`.
///
/// GraphQL answers `200` with an `errors` array: with no usable `data` the first error is mapped
/// (`RATE_LIMITED` → `RateLimited`, `NOT_FOUND` → `NotFound`, `FORBIDDEN` → `PermissionDenied`,
/// `INSUFFICIENT_SCOPES` → `PermissionDenied`, anything else → `Upstream`). Partial data with
/// errors is returned as is, unless every top-level field is null.
pub async fn graphql(auth: &Authed, url: &str, query: &str, variables: Value) -> Result<Value, KeltaError> {
    let req = HttpRequest::post(url).json(json!({ "query": query, "variables": variables }));
    let resp = auth.send_json::<Value>(req).await?.body;
    let data = resp.get("data").filter(|d| !d.is_null()).cloned();
    let errors = resp.get("errors").and_then(Value::as_array).filter(|e| !e.is_empty());
    // A rejected mutation or hidden owner answers `{"x": null}` plus errors: that is a failure too.
    let all_null = |d: &Value| d.as_object().is_some_and(|o| o.values().all(Value::is_null));
    match (data, errors) {
        (Some(d), Some(errs)) if all_null(&d) => Err(map_graphql_error(&errs[0])),
        (Some(d), _) => Ok(d),
        (None, Some(errs)) => Err(map_graphql_error(&errs[0])),
        (None, None) => Err(KeltaError::upstream("graphql response without data")),
    }
}

fn map_graphql_error(e: &Value) -> KeltaError {
    let msg = e.get("message").and_then(Value::as_str).unwrap_or("graphql error");
    let ty = e.get("type").and_then(Value::as_str).unwrap_or("");
    let code = match ty {
        "RATE_LIMITED" => ErrorCode::RateLimited,
        "NOT_FOUND" => ErrorCode::NotFound,
        "FORBIDDEN" | "INSUFFICIENT_SCOPES" => ErrorCode::PermissionDenied,
        _ => ErrorCode::Upstream,
    };
    KeltaError::new(code, format!("GraphQL {ty}: {msg}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn maps_error_types() {
        assert_eq!(
            map_graphql_error(&json!({"type": "RATE_LIMITED", "message": "x"})).code,
            ErrorCode::RateLimited
        );
        assert_eq!(map_graphql_error(&json!({"type": "NOT_FOUND"})).code, ErrorCode::NotFound);
        assert_eq!(map_graphql_error(&json!({"message": "boom"})).code, ErrorCode::Upstream);
    }
}
