//! Custom Deno ops for Perry runtime integration
//!
//! These ops allow JavaScript code to call back into native Perry code.

use deno_core::{extension, op2};
use deno_error::JsErrorBox;
use std::collections::HashMap;

#[op2]
#[string]
fn op_perry_log(#[string] message: String) -> String {
    log::info!("[JS] {}", message);
    message
}

#[op2]
#[serde]
fn op_perry_call_native(
    #[string] func_name: String,
    #[serde] args: Vec<serde_json::Value>,
) -> serde_json::Value {
    log::debug!("Native call: {} with {} args", func_name, args.len());
    // TODO: Look up function in registry and call it
    serde_json::Value::Null
}

/// Synchronous HTTP fetch op for V8's fetch() polyfill.
/// Uses ureq (blocking) to avoid Tokio runtime conflicts when called
/// from within js_await_js_promise's block_on context.
#[op2]
#[serde]
fn op_perry_fetch(
    #[string] url: String,
    #[string] method: String,
    #[string] body: String,
    #[serde] headers: HashMap<String, String>,
) -> Result<serde_json::Value, JsErrorBox> {
    let agent = ureq::agent();
    let method_upper = method.to_uppercase();

    let mut req = agent.request(&method_upper, &url);

    for (key, value) in &headers {
        req = req.set(key, value);
    }

    let resp = if !body.is_empty() {
        req.set("Content-Type", "application/json")
            .send_string(&body)
    } else {
        req.call()
    };

    match resp {
        Ok(resp) => {
            let status = resp.status();
            let status_text = resp.status_text().to_string();

            let mut resp_headers = serde_json::Map::new();
            for name in resp.headers_names() {
                if let Some(value) = resp.header(&name) {
                    resp_headers.insert(
                        name.to_string(),
                        serde_json::Value::String(value.to_string()),
                    );
                }
            }

            let resp_body = resp.into_string().unwrap_or_default();

            Ok(serde_json::json!({
                "status": status,
                "statusText": status_text,
                "headers": resp_headers,
                "body": resp_body,
            }))
        }
        Err(ureq::Error::Status(code, resp)) => {
            let resp_body = resp.into_string().unwrap_or_default();
            Ok(serde_json::json!({
                "status": code,
                "statusText": "Error",
                "headers": {},
                "body": resp_body,
            }))
        }
        Err(e) => Err(JsErrorBox::generic(format!("fetch error: {}", e))),
    }
}

extension!(
    perry_ops,
    ops = [op_perry_log, op_perry_call_native, op_perry_fetch,],
);
