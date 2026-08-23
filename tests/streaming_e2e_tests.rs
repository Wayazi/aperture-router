// SPDX-License-Identifier: MIT
// Copyright (c) 2026 aperture-router contributors

//! End-to-end SSE streaming tests through the real router stack.
//!
//! Each test spawns a raw tokio TCP server as the upstream so the test
//! controls exact byte/chunk boundaries — something wiremock cannot do —
//! and asserts on the Anthropic SSE events the router emits.

mod common;

use common::{add_connect_info, create_test_router};

use axum::{
    body::Body,
    http::{Method, Request, StatusCode},
};
use http_body_util::BodyExt;
use std::time::Duration;
use tokio::io::AsyncWriteExt;
use tower::ServiceExt;

use aperture_router::{config::Config, discovery::models::ModelDiscovery};

/// Spawn an upstream HTTP server that writes `response_bytes` split into
/// the given chunk sizes (simulating TCP segmentation), then closes the
/// connection. Serves up to `connections` sequential connections (discovery
/// or retries may open their own). Returns the bound address.
async fn spawn_chunked_upstream(response_bytes: &[u8], chunk_sizes: &[usize]) -> String {
    spawn_chunked_upstream_n(response_bytes, chunk_sizes, 4).await
}

async fn spawn_chunked_upstream_n(
    response_bytes: &[u8],
    chunk_sizes: &[usize],
    connections: usize,
) -> String {
    use tokio::net::TcpListener;

    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap().to_string();

    let mut body: Vec<Vec<u8>> = Vec::new();
    let mut offset = 0usize;
    for size in chunk_sizes {
        let end = (offset + size).min(response_bytes.len());
        if offset < response_bytes.len() {
            body.push(response_bytes[offset..end].to_vec());
        }
        offset = end;
    }
    if offset < response_bytes.len() {
        body.push(response_bytes[offset..].to_vec());
    }

    let body_len = response_bytes.len();
    let headers = format!(
        "HTTP/1.1 200 OK\r\ncontent-type: text/event-stream\r\ncontent-length: {}\r\nconnection: close\r\n\r\n",
        body_len
    );

    let body = std::sync::Arc::new(body);
    let headers = std::sync::Arc::new(headers);
    tokio::spawn(async move {
        for _ in 0..connections {
            let (socket, _) = match listener.accept().await {
                Ok(c) => c,
                Err(_) => break,
            };
            let mut socket = socket;
            let _ = socket.write_all(headers.as_bytes()).await;
            for chunk in body.iter() {
                let _ = socket.write_all(chunk).await;
                let _ = socket.flush().await;
                // Small delay forces separate TCP deliveries.
                tokio::time::sleep(Duration::from_millis(5)).await;
            }
            let _ = socket.shutdown().await;
        }
    });

    format!("http://{}", addr)
}

fn router_with_upstream(upstream_url: String) -> axum::Router {
    let mut config = Config::default();
    config.aperture.base_url = upstream_url;
    config.security.require_auth_in_prod = false;
    let discovery = ModelDiscovery::new(config.aperture.clone(), &config.http).unwrap();
    create_test_router(config, std::sync::Arc::new(discovery))
}

async fn post_stream(app: axum::Router, body: &str) -> (StatusCode, String) {
    let request = Request::builder()
        .uri("/v1/messages")
        .method(Method::POST)
        .header("content-type", "application/json")
        .body(Body::from(body.to_string()))
        .unwrap();

    let response = app.oneshot(add_connect_info(request)).await.unwrap();
    let status = response.status();
    let collected = response.into_body().collect().await.unwrap().to_bytes();
    (status, String::from_utf8_lossy(&collected).to_string())
}

fn openai_sse(chunks: &[serde_json::Value]) -> Vec<u8> {
    let mut out = String::new();
    for c in chunks {
        out.push_str(&format!("data: {}\n\n", c));
    }
    out.push_str("data: [DONE]\n\n");
    out.into_bytes()
}

#[tokio::test]
async fn test_sse_line_split_across_tcp_chunks_is_reassembled() {
    // One data line whose JSON is cut mid-string between two chunks. Before
    // SseLineBuffer this produced a truncated event + dropped continuation.
    let full_line = r#"{"id":"1","object":"chat.completion.chunk","model":"t","choices":[{"index":0,"delta":{"content":"Hello world"}}]}"#;
    let split_at = full_line.len() - 12; // cut inside "Hello world"
                                         // The wire bytes are ONE line: `data: <json>\n\n` — the chunk boundary
                                         // falls mid-JSON with NO newline inserted at the split point.
    let mut wire = Vec::new();
    wire.extend_from_slice(b"data: ");
    wire.extend_from_slice(full_line.as_bytes());
    wire.extend_from_slice(b"\n\n");
    wire.extend_from_slice(b"data: [DONE]\n\n");

    // Deliver in two writes, the first ending exactly at the JSON cut.
    let upstream = spawn_chunked_upstream(&wire, &[split_at]).await;
    let app = router_with_upstream(upstream);

    let (status, sse_out) = post_stream(
        app,
        r#"{"model":"test-model","max_tokens":50,"stream":true,"messages":[{"role":"user","content":"hi"}]}"#,
    )
    .await;

    assert_eq!(status, StatusCode::OK);
    if !sse_out.contains("Hello world") {
        panic!(
            "split line not reassembled.\n--- SSE OUT ({} bytes) ---\n{}\n--- END ---",
            sse_out.len(),
            sse_out
        );
    }
    // No truncated-JSON fragments should survive as their own events.
    assert!(
        !sse_out.contains("lo world\"}}"),
        "no partial-line fragment may be forwarded; got: {}",
        sse_out
    );
}

#[tokio::test]
async fn test_thinking_block_emitted_before_text() {
    let chunks = vec![
        serde_json::json!({"id":"1","object":"chat.completion.chunk","model":"t","choices":[{"index":0,"delta":{"role":"assistant","reasoning":"Thinking..."}}]}),
        serde_json::json!({"id":"1","object":"chat.completion.chunk","model":"t","choices":[{"index":0,"delta":{"content":"ok"}}]}),
        serde_json::json!({"id":"1","object":"chat.completion.chunk","model":"t","choices":[{"index":0,"delta":{},"finish_reason":"stop"}]}),
    ];
    let upstream = spawn_chunked_upstream(&openai_sse(&chunks), &[4096]).await;
    let app = router_with_upstream(upstream);

    let (status, sse_out) = post_stream(
        app,
        r#"{"model":"test-model","max_tokens":100,"stream":true,"messages":[{"role":"user","content":"hi"}]}"#,
    )
    .await;

    assert_eq!(status, StatusCode::OK);
    let thinking_pos = sse_out
        .find(r#""type":"thinking""#)
        .or_else(|| sse_out.find("\"thinking\""));
    let text_pos = sse_out
        .find(r#""type":"text""#)
        .or_else(|| sse_out.find("\"text\""));
    assert!(
        thinking_pos.is_some(),
        "thinking block expected; got {}",
        sse_out
    );
    assert!(text_pos.is_some(), "text block expected; got {}", sse_out);
    assert!(
        thinking_pos.unwrap() < text_pos.unwrap(),
        "thinking block must precede text block; got {}",
        sse_out
    );
}

#[tokio::test]
async fn test_parallel_tool_calls_get_distinct_blocks() {
    let chunks = vec![
        serde_json::json!({"id":"1","object":"chat.completion.chunk","model":"t","choices":[{"index":0,"delta":{"tool_calls":[{"index":0,"id":"call_a","type":"function","function":{"name":"fa","arguments":""}}]}}]}),
        serde_json::json!({"id":"1","object":"chat.completion.chunk","model":"t","choices":[{"index":0,"delta":{"tool_calls":[{"index":1,"id":"call_b","type":"function","function":{"name":"fb","arguments":""}}]}}]}),
        serde_json::json!({"id":"1","object":"chat.completion.chunk","model":"t","choices":[{"index":0,"delta":{},"finish_reason":"tool_calls"}]}),
    ];
    let upstream = spawn_chunked_upstream(&openai_sse(&chunks), &[4096]).await;
    let app = router_with_upstream(upstream);

    let (status, sse_out) = post_stream(
        app,
        r#"{"model":"test-model","max_tokens":100,"stream":true,"tools":[{"name":"fa","input_schema":{"type":"object"}}],"messages":[{"role":"user","content":"hi"}]}"#,
    )
    .await;

    assert_eq!(status, StatusCode::OK);
    let count = sse_out.matches(r#""type":"tool_use""#).count();
    assert_eq!(
        count, 2,
        "two tool calls must produce two distinct tool_use blocks; got {}",
        sse_out
    );
    assert!(sse_out.contains("call_a") && sse_out.contains("call_b"));
}
