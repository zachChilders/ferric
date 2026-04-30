//! # `llm` — Minimal LLM client (OpenRouter, OpenAI-compatible).
//!
//! Tiny on purpose. Two functions:
//!   * `llm::prompt(text: Str) -> Result<Str, LlmError>`
//!     Sends a single user message, returns the assistant's text reply.
//!   * `llm::prompt_schema(text: Str, schema: Json) -> Result<Json, LlmError>`
//!     Same, but instructs the model to produce JSON matching `schema`.
//!     Uses the OpenAI-compatible `response_format: json_schema` field.
//!
//! Configuration is read from environment variables (load `.env` first via
//! the `dotenv` module if you like):
//!
//!   * `OPENROUTER_API_KEY`  (required)
//!   * `OPENROUTER_BASE_URL` (default: `https://openrouter.ai/api/v1`)
//!   * `OPENROUTER_MODEL`    (default: `anthropic/claude-sonnet-4-7`)
//!   * `OPENROUTER_TIMEOUT_MS` (default: `60000`)
//!
//! No streaming, no message history, no tool calls. Those land in a richer
//! `llm` API later; this is the 80% surface for one-shot prompting.

use std::sync::OnceLock;
use std::time::Duration;

use ferric_common::Interner;
use reqwest::blocking::Client;

use crate::{JsonRepr, NativeRegistry, NativeValue};

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn check_arg_count(args: &[NativeValue], expected: usize) -> Result<(), String> {
    if args.len() != expected {
        Err(format!(
            "expected {expected} argument(s), got {}",
            args.len()
        ))
    } else {
        Ok(())
    }
}

fn expect_str(value: &NativeValue) -> Result<&String, String> {
    match value {
        NativeValue::Str(s) => Ok(s),
        other => Err(format!("expected string, got {other:?}")),
    }
}

fn expect_json(value: &NativeValue) -> Result<&JsonRepr, String> {
    match value {
        NativeValue::Json(j) => Ok(j),
        other => Err(format!("expected Json, got {other:?}")),
    }
}

fn err_result(msg: impl Into<String>) -> NativeValue {
    NativeValue::Result(Box::new(Err(NativeValue::Str(msg.into()))))
}

fn ok_result(v: NativeValue) -> NativeValue {
    NativeValue::Result(Box::new(Ok(v)))
}

// ---------------------------------------------------------------------------
// Configuration
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
struct Config {
    api_key: String,
    base_url: String,
    model: String,
    timeout: Duration,
}

fn load_config() -> Result<Config, String> {
    let api_key = std::env::var("OPENROUTER_API_KEY")
        .map_err(|_| "LlmError::MissingApiKey: OPENROUTER_API_KEY not set".to_string())?;
    if api_key.trim().is_empty() {
        return Err("LlmError::MissingApiKey: OPENROUTER_API_KEY is empty".to_string());
    }
    let base_url = std::env::var("OPENROUTER_BASE_URL")
        .unwrap_or_else(|_| "https://openrouter.ai/api/v1".to_string());
    let model = std::env::var("OPENROUTER_MODEL")
        .unwrap_or_else(|_| "anthropic/claude-sonnet-4-7".to_string());
    let timeout_ms: u64 = std::env::var("OPENROUTER_TIMEOUT_MS")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(60_000);
    Ok(Config {
        api_key,
        base_url: base_url.trim_end_matches('/').to_string(),
        model,
        timeout: Duration::from_millis(timeout_ms),
    })
}

fn shared_client() -> &'static Client {
    static CLIENT: OnceLock<Client> = OnceLock::new();
    CLIENT.get_or_init(|| {
        Client::builder()
            .build()
            .expect("failed to build llm reqwest::blocking::Client")
    })
}

// ---------------------------------------------------------------------------
// Hand-rolled JSON serialisation for the request body.
//
// Avoids dragging in `serde_json` just for this one boundary. We already
// own a `JsonRepr` from the `json` module; we write a tiny serialiser here
// because the request body is a fixed shape.
// ---------------------------------------------------------------------------

fn escape_string(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('"');
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => {
                out.push_str(&format!("\\u{:04x}", c as u32));
            }
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

fn json_repr_to_string(v: &JsonRepr) -> String {
    crate::json::to_str(v)
}

fn build_request_body(prompt: &str, model: &str, schema: Option<&JsonRepr>) -> String {
    let mut out = String::new();
    out.push_str("{\"model\":");
    out.push_str(&escape_string(model));
    out.push_str(",\"messages\":[{\"role\":\"user\",\"content\":");
    out.push_str(&escape_string(prompt));
    out.push_str("}]");
    if let Some(s) = schema {
        out.push_str(",\"response_format\":{\"type\":\"json_schema\",\"json_schema\":{\"name\":\"response\",\"strict\":true,\"schema\":");
        out.push_str(&json_repr_to_string(s));
        out.push_str("}}");
    }
    out.push('}');
    out
}

// ---------------------------------------------------------------------------
// Response parsing
// ---------------------------------------------------------------------------

/// Extracts `choices[0].message.content` from an OpenAI-shaped response.
fn extract_content(resp_json: &JsonRepr) -> Result<String, String> {
    let JsonRepr::Object(root) = resp_json else {
        return Err("LlmError::InvalidResponse: top-level is not an object".to_string());
    };
    let choices = lookup(root, "choices")
        .ok_or_else(|| "LlmError::InvalidResponse: missing 'choices'".to_string())?;
    let JsonRepr::Array(arr) = choices else {
        return Err("LlmError::InvalidResponse: 'choices' is not an array".to_string());
    };
    let first = arr
        .first()
        .ok_or_else(|| "LlmError::InvalidResponse: 'choices' is empty".to_string())?;
    let JsonRepr::Object(first_obj) = first else {
        return Err("LlmError::InvalidResponse: choices[0] is not an object".to_string());
    };
    let message = lookup(first_obj, "message")
        .ok_or_else(|| "LlmError::InvalidResponse: missing 'message'".to_string())?;
    let JsonRepr::Object(msg_obj) = message else {
        return Err("LlmError::InvalidResponse: 'message' is not an object".to_string());
    };
    let content = lookup(msg_obj, "content")
        .ok_or_else(|| "LlmError::InvalidResponse: missing 'content'".to_string())?;
    match content {
        JsonRepr::Str(s) => Ok(s.clone()),
        _ => Err("LlmError::InvalidResponse: 'content' is not a string".to_string()),
    }
}

fn lookup<'a>(obj: &'a [(String, JsonRepr)], key: &str) -> Option<&'a JsonRepr> {
    obj.iter().find(|(k, _)| k == key).map(|(_, v)| v)
}

// ---------------------------------------------------------------------------
// Send
// ---------------------------------------------------------------------------

fn send(prompt: &str, schema: Option<&JsonRepr>) -> Result<String, String> {
    let cfg = load_config()?;
    let body = build_request_body(prompt, &cfg.model, schema);
    let url = format!("{}/chat/completions", cfg.base_url);

    let resp = shared_client()
        .post(&url)
        .header("Authorization", format!("Bearer {}", cfg.api_key))
        .header("Content-Type", "application/json")
        .timeout(cfg.timeout)
        .body(body)
        .send();

    let resp = match resp {
        Ok(r) => r,
        Err(e) if e.is_timeout() => return Err(format!("LlmError::Timeout: {e}")),
        Err(e) => return Err(format!("LlmError::HttpError: {e}")),
    };

    let status = resp.status().as_u16();
    let body_text = resp
        .text()
        .map_err(|e| format!("LlmError::InvalidResponse: read body: {e}"))?;

    if !(200..300).contains(&status) {
        return Err(format!("LlmError::ApiError: status {status}: {body_text}"));
    }

    let parsed = crate::json::parse(&body_text)
        .map_err(|e| format!("LlmError::InvalidResponse: parse JSON: {e:?}"))?;
    extract_content(&parsed)
}

// ---------------------------------------------------------------------------
// Native bindings
// ---------------------------------------------------------------------------

/// `llm::prompt(text: Str) -> Result<Str, LlmError>`
fn builtin_llm_prompt(args: &[NativeValue]) -> Result<NativeValue, String> {
    check_arg_count(args, 1)?;
    let text = expect_str(&args[0])?;
    match send(text, None) {
        Ok(s) => Ok(ok_result(NativeValue::Str(s))),
        Err(e) => Ok(err_result(e)),
    }
}

/// `llm::prompt_schema(text: Str, schema: Json) -> Result<Json, LlmError>` —
/// returns the model's reply, parsed as JSON.
fn builtin_llm_prompt_schema(args: &[NativeValue]) -> Result<NativeValue, String> {
    check_arg_count(args, 2)?;
    let text = expect_str(&args[0])?;
    let schema = expect_json(&args[1])?;
    match send(text, Some(schema)) {
        Ok(s) => match crate::json::parse(&s) {
            Ok(j) => Ok(ok_result(NativeValue::Json(Box::new(j)))),
            Err(e) => Ok(err_result(format!(
                "LlmError::InvalidResponse: model returned non-JSON content: {e:?}"
            ))),
        },
        Err(e) => Ok(err_result(e)),
    }
}

// ---------------------------------------------------------------------------
// Registration
// ---------------------------------------------------------------------------

pub fn register(registry: &mut NativeRegistry, interner: &mut Interner) {
    let bodies: &[(&str, crate::NativeFn)] = &[
        ("llm_prompt", builtin_llm_prompt),
        ("llm_prompt_schema", builtin_llm_prompt_schema),
    ];
    crate::register_module(
        registry,
        interner,
        ferric_stdlib_meta::llm::LLM_FNS,
        bodies,
    );
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    /// Guard that sets/clears env vars for one test. Restores prior values
    /// on drop so tests don't leak.
    struct EnvGuard {
        keys: Vec<(String, Option<String>)>,
    }

    impl EnvGuard {
        fn new() -> Self {
            Self { keys: Vec::new() }
        }
        fn set(&mut self, k: &str, v: &str) {
            self.keys.push((k.to_string(), std::env::var(k).ok()));
            unsafe {
                std::env::set_var(k, v);
            }
        }
        fn clear(&mut self, k: &str) {
            self.keys.push((k.to_string(), std::env::var(k).ok()));
            unsafe {
                std::env::remove_var(k);
            }
        }
    }

    impl Drop for EnvGuard {
        fn drop(&mut self) {
            for (k, prev) in &self.keys {
                unsafe {
                    match prev {
                        Some(v) => std::env::set_var(k, v),
                        None => std::env::remove_var(k),
                    }
                }
            }
        }
    }

    #[test]
    fn missing_api_key_yields_typed_error() {
        let mut g = EnvGuard::new();
        g.clear("OPENROUTER_API_KEY");
        let r = builtin_llm_prompt(&[NativeValue::Str("hi".into())]).unwrap();
        match r {
            NativeValue::Result(b) => match *b {
                Err(NativeValue::Str(s)) => {
                    assert!(s.contains("LlmError::MissingApiKey"), "got: {s}")
                }
                _ => panic!(),
            },
            _ => panic!(),
        }
    }

    #[test]
    fn build_request_body_basic() {
        let body = build_request_body("hello", "gpt-test", None);
        assert!(body.contains("\"model\":\"gpt-test\""));
        assert!(body.contains("\"role\":\"user\""));
        assert!(body.contains("\"content\":\"hello\""));
        assert!(!body.contains("response_format"));
    }

    #[test]
    fn build_request_body_with_schema() {
        let schema = JsonRepr::Object(vec![("type".to_string(), JsonRepr::Str("object".into()))]);
        let body = build_request_body("hi", "m", Some(&schema));
        assert!(body.contains("response_format"));
        assert!(body.contains("\"json_schema\""));
        assert!(body.contains("\"strict\":true"));
        assert!(body.contains("\"type\":\"object\""));
    }

    #[test]
    fn build_request_body_escapes_newlines_and_quotes() {
        let body = build_request_body("say \"hi\"\nokay", "m", None);
        assert!(body.contains("say \\\"hi\\\"\\nokay"));
    }

    #[test]
    fn extract_content_happy_path() {
        let r = JsonRepr::Object(vec![(
            "choices".to_string(),
            JsonRepr::Array(vec![JsonRepr::Object(vec![(
                "message".to_string(),
                JsonRepr::Object(vec![(
                    "content".to_string(),
                    JsonRepr::Str("hello there".into()),
                )]),
            )])]),
        )]);
        assert_eq!(extract_content(&r).unwrap(), "hello there");
    }

    #[test]
    fn extract_content_missing_field() {
        let r = JsonRepr::Object(vec![]);
        let e = extract_content(&r).unwrap_err();
        assert!(e.contains("LlmError::InvalidResponse"));
    }

    #[test]
    fn end_to_end_against_mock_server() {
        // Spawn a tiny_http server impersonating OpenRouter. Verify that
        // the request body shape is correct and that the response content
        // is propagated back through `builtin_llm_prompt`.
        let server = tiny_http::Server::http("127.0.0.1:0").expect("bind");
        let port = server.server_addr().to_ip().unwrap().port();
        let base = format!("http://127.0.0.1:{port}");

        let captured_body = std::sync::Arc::new(std::sync::Mutex::new(String::new()));
        let captured_auth = std::sync::Arc::new(std::sync::Mutex::new(String::new()));
        let captured_path = std::sync::Arc::new(std::sync::Mutex::new(String::new()));
        let cb = captured_body.clone();
        let ca = captured_auth.clone();
        let cp = captured_path.clone();
        let server_arc = std::sync::Arc::new(server);
        let server_clone = server_arc.clone();
        let join = std::thread::spawn(move || {
            if let Ok(mut req) = server_clone.recv() {
                *cp.lock().unwrap() = req.url().to_string();
                let auth = req
                    .headers()
                    .iter()
                    .find(|h| {
                        h.field
                            .as_str()
                            .as_str()
                            .eq_ignore_ascii_case("Authorization")
                    })
                    .map(|h| h.value.as_str().to_string())
                    .unwrap_or_default();
                *ca.lock().unwrap() = auth;
                let mut body = String::new();
                let _ = std::io::Read::read_to_string(req.as_reader(), &mut body);
                *cb.lock().unwrap() = body;
                let resp = tiny_http::Response::from_string(
                    r#"{"choices":[{"message":{"role":"assistant","content":"hi from mock"}}]}"#,
                )
                .with_header(
                    "Content-Type: application/json"
                        .parse::<tiny_http::Header>()
                        .unwrap(),
                );
                let _ = req.respond(resp);
            }
        });

        let mut g = EnvGuard::new();
        g.set("OPENROUTER_API_KEY", "test-key");
        g.set("OPENROUTER_BASE_URL", &base);
        g.set("OPENROUTER_MODEL", "test-model");

        let r = builtin_llm_prompt(&[NativeValue::Str("ping".into())]).unwrap();
        match r {
            NativeValue::Result(b) => match *b {
                Ok(NativeValue::Str(s)) => assert_eq!(s, "hi from mock"),
                Err(e) => panic!("expected Ok, got Err({e:?})"),
                _ => panic!(),
            },
            _ => panic!(),
        }

        // Confirm the captured request contents.
        let body = captured_body.lock().unwrap().clone();
        assert!(body.contains("\"model\":\"test-model\""));
        assert!(body.contains("\"content\":\"ping\""));
        let auth = captured_auth.lock().unwrap().clone();
        assert_eq!(auth, "Bearer test-key");
        let path = captured_path.lock().unwrap().clone();
        assert_eq!(path, "/chat/completions");

        server_arc.unblock();
        let _ = join.join();
    }

    #[test]
    fn end_to_end_with_schema_parses_json_response() {
        let server = tiny_http::Server::http("127.0.0.1:0").expect("bind");
        let port = server.server_addr().to_ip().unwrap().port();
        let base = format!("http://127.0.0.1:{port}");
        let captured_body = std::sync::Arc::new(std::sync::Mutex::new(String::new()));
        let cb = captured_body.clone();
        let server_arc = std::sync::Arc::new(server);
        let server_clone = server_arc.clone();
        let join = std::thread::spawn(move || {
            if let Ok(mut req) = server_clone.recv() {
                let mut body = String::new();
                let _ = std::io::Read::read_to_string(req.as_reader(), &mut body);
                *cb.lock().unwrap() = body;
                let resp = tiny_http::Response::from_string(
                    r#"{"choices":[{"message":{"content":"{\"answer\":42}"}}]}"#,
                );
                let _ = req.respond(resp);
            }
        });

        let mut g = EnvGuard::new();
        g.set("OPENROUTER_API_KEY", "k");
        g.set("OPENROUTER_BASE_URL", &base);
        g.set("OPENROUTER_MODEL", "m");

        let schema = NativeValue::Json(Box::new(JsonRepr::Object(vec![(
            "type".to_string(),
            JsonRepr::Str("object".into()),
        )])));
        let r = builtin_llm_prompt_schema(&[NativeValue::Str("structured plz".into()), schema])
            .unwrap();
        match r {
            NativeValue::Result(b) => match *b {
                Ok(NativeValue::Json(j)) => {
                    let JsonRepr::Object(fields) = *j else {
                        panic!("expected JSON object");
                    };
                    assert_eq!(
                        fields.iter().find(|(k, _)| k == "answer").map(|(_, v)| v),
                        Some(&JsonRepr::Int(42))
                    );
                }
                _ => panic!("expected Ok(Json)"),
            },
            _ => panic!(),
        }
        // The outgoing body must contain the schema block.
        let body = captured_body.lock().unwrap().clone();
        assert!(body.contains("response_format"));
        assert!(body.contains("\"strict\":true"));

        server_arc.unblock();
        let _ = join.join();
    }

    #[test]
    fn api_error_status_surfaces() {
        let server = tiny_http::Server::http("127.0.0.1:0").expect("bind");
        let port = server.server_addr().to_ip().unwrap().port();
        let base = format!("http://127.0.0.1:{port}");
        let server_arc = std::sync::Arc::new(server);
        let server_clone = server_arc.clone();
        let join = std::thread::spawn(move || {
            if let Ok(req) = server_clone.recv() {
                let resp = tiny_http::Response::from_string("rate limited").with_status_code(429);
                let _ = req.respond(resp);
            }
        });

        let mut g = EnvGuard::new();
        g.set("OPENROUTER_API_KEY", "k");
        g.set("OPENROUTER_BASE_URL", &base);

        let r = builtin_llm_prompt(&[NativeValue::Str("hi".into())]).unwrap();
        match r {
            NativeValue::Result(b) => match *b {
                Err(NativeValue::Str(s)) => {
                    assert!(s.contains("LlmError::ApiError"), "got: {s}");
                    assert!(s.contains("429"));
                }
                _ => panic!(),
            },
            _ => panic!(),
        }

        server_arc.unblock();
        let _ = join.join();
    }

    #[test]
    fn register_installs_every_function() {
        let mut registry = NativeRegistry::new();
        let mut interner = Interner::new();
        register(&mut registry, &mut interner);
        for name in ["llm_prompt", "llm_prompt_schema"] {
            assert!(
                registry.get(interner.intern(name)).is_some(),
                "missing: {name}"
            );
        }
    }
}
