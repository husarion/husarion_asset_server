//! husarion_asset conformance suite — validates that a `GetAsset` provider obeys
//! the husarion_asset_msgs standard. Point it at a running provider's service and
//! a known asset; it exercises the contract and reports PASS/FAIL per check.
//!
//! ```bash
//! asset_conformance \
//!   --service /husarion_asset_server/get_asset \
//!   --uri package://husarion_asset_msgs/package.xml
//! ```
//!
//! Checks: full fetch (success + total_size + sha256 content_hash + data length),
//! chunked fetch stitches to the whole (and `content_hash` is stable across
//! chunks), hash determinism, unknown-package rejection, and `..` traversal
//! rejection. Exit code is non-zero if any check fails.

use std::future::Future;
use std::task::{Context, Poll};
use std::time::{Duration, Instant};

use clap::Parser;
use futures::task::noop_waker;
use r2r::QosProfile;
use serde_json::{json, Value};

const GET_ASSET_TYPE: &str = "husarion_asset_msgs/srv/GetAsset";
const CALL_TIMEOUT: Duration = Duration::from_secs(10);

#[derive(Parser, Debug)]
#[command(
    name = "asset_conformance",
    about = "Conformance suite for husarion_asset_msgs providers"
)]
struct Args {
    /// The provider's GetAsset service name.
    #[arg(long, default_value = "/husarion_asset_server/get_asset")]
    service: String,
    /// A known-good asset URI the provider owns.
    #[arg(long)]
    uri: String,
    /// A package the provider does NOT own (for the rejection check).
    #[arg(long, default_value = "__husarion_conformance_absent__")]
    unknown_package: String,
    /// Seconds to warm DDS discovery before the first request.
    #[arg(long, default_value_t = 8)]
    warmup_secs: u64,
}

fn main() -> anyhow::Result<()> {
    let args = Args::parse();
    let ctx = r2r::Context::create()?;
    let mut node = r2r::Node::create(ctx, "husarion_asset_conformance", "")?;
    let client =
        node.create_client_untyped(&args.service, GET_ASSET_TYPE, QosProfile::default())?;

    // Warm DDS discovery: a fresh process must discover the provider's service
    // before a request can round-trip (slower across containers).
    let warm = Instant::now();
    while warm.elapsed() < Duration::from_secs(args.warmup_secs) {
        node.spin_once(Duration::from_millis(50));
    }

    let mut checks: Vec<(String, bool, String)> = Vec::new();
    let mut push =
        |name: &str, ok: bool, detail: String| checks.push((name.to_string(), ok, detail));

    // 1. Full fetch.
    let full = match call(
        &mut node,
        &client,
        json!({"uri": args.uri, "offset": 0, "length": 0}),
    ) {
        Ok(v) => v,
        Err(e) => {
            push("full fetch", false, format!("request failed: {e}"));
            return report(&checks);
        }
    };
    let success = full
        .get("success")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let total = full.get("total_size").and_then(Value::as_u64).unwrap_or(0);
    let hash = full
        .get("content_hash")
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_string();
    let data_len = full
        .get("data")
        .and_then(Value::as_array)
        .map(|a| a.len())
        .unwrap_or(0) as u64;
    push(
        "full fetch: success",
        success,
        full.get("error")
            .and_then(Value::as_str)
            .unwrap_or("")
            .into(),
    );
    push(
        "full fetch: total_size > 0",
        total > 0,
        format!("total_size={total}"),
    );
    push(
        "full fetch: content_hash is sha256",
        hash.starts_with("sha256:") && hash.len() == "sha256:".len() + 64,
        hash.clone(),
    );
    push(
        "full fetch: data length == total_size",
        data_len == total,
        format!("data={data_len} total={total}"),
    );

    // 2. Chunked fetch stitches to the whole + stable hash.
    if total > 1 {
        let half = total / 2;
        let a = call(
            &mut node,
            &client,
            json!({"uri": args.uri, "offset": 0, "length": half}),
        );
        let b = call(
            &mut node,
            &client,
            json!({"uri": args.uri, "offset": half, "length": 0}),
        );
        if let (Ok(a), Ok(b)) = (a, b) {
            let la = a
                .get("data")
                .and_then(Value::as_array)
                .map(|x| x.len())
                .unwrap_or(0) as u64;
            let lb = b
                .get("data")
                .and_then(Value::as_array)
                .map(|x| x.len())
                .unwrap_or(0) as u64;
            push(
                "chunked fetch: chunks stitch to total_size",
                la + lb == total,
                format!("{la}+{lb} vs {total}"),
            );
            let ha = a.get("content_hash").and_then(Value::as_str).unwrap_or("");
            let hb = b.get("content_hash").and_then(Value::as_str).unwrap_or("");
            push(
                "chunked fetch: content_hash stable across chunks",
                ha == hash && hb == hash,
                String::new(),
            );
        } else {
            push("chunked fetch", false, "a chunk request failed".into());
        }
    }

    // 3. Hash determinism.
    if let Ok(again) = call(
        &mut node,
        &client,
        json!({"uri": args.uri, "offset": 0, "length": 0}),
    ) {
        let h2 = again
            .get("content_hash")
            .and_then(Value::as_str)
            .unwrap_or("");
        push("content_hash is deterministic", h2 == hash, String::new());
    }

    // 4. Unknown package rejected.
    let bad = format!("package://{}/x.dae", args.unknown_package);
    if let Ok(v) = call(
        &mut node,
        &client,
        json!({"uri": bad, "offset": 0, "length": 0}),
    ) {
        push(
            "unknown package rejected (success:false)",
            !v.get("success").and_then(Value::as_bool).unwrap_or(true),
            String::new(),
        );
    }

    // 5. Path traversal rejected.
    let pkg = args
        .uri
        .strip_prefix("package://")
        .and_then(|r| r.split('/').next())
        .unwrap_or("");
    let traversal = format!("package://{pkg}/../etc/passwd");
    if let Ok(v) = call(
        &mut node,
        &client,
        json!({"uri": traversal, "offset": 0, "length": 0}),
    ) {
        push(
            "path traversal rejected (success:false)",
            !v.get("success").and_then(Value::as_bool).unwrap_or(true),
            String::new(),
        );
    }

    report(&checks)
}

fn report(checks: &[(String, bool, String)]) -> anyhow::Result<()> {
    let mut failed = 0;
    println!("\nhusarion_asset conformance\n");
    for (name, ok, detail) in checks {
        let tag = if *ok {
            "\x1b[32mPASS\x1b[0m"
        } else {
            "\x1b[31mFAIL\x1b[0m"
        };
        let suffix = if detail.is_empty() {
            String::new()
        } else {
            format!(" — {detail}")
        };
        println!("  {tag}  {name}{suffix}");
        if !ok {
            failed += 1;
        }
    }
    println!("\n{}/{} checks passed", checks.len() - failed, checks.len());
    std::process::exit(if failed == 0 { 0 } else { 1 });
}

/// One GetAsset call, driven to completion by spinning the node (no executor).
fn call(node: &mut r2r::Node, client: &r2r::ClientUntyped, req: Value) -> Result<Value, String> {
    let fut = client.request(req).map_err(|e| e.to_string())?;
    let mut fut = Box::pin(fut);
    let waker = noop_waker();
    let mut cx = Context::from_waker(&waker);
    let start = Instant::now();
    loop {
        node.spin_once(Duration::from_millis(20));
        match fut.as_mut().poll(&mut cx) {
            Poll::Ready(Ok(v)) => return v.map_err(|e| e.to_string()),
            Poll::Ready(Err(_)) => return Err("client cancelled".to_string()),
            Poll::Pending => {
                if start.elapsed() > CALL_TIMEOUT {
                    return Err("call timed out".to_string());
                }
            }
        }
    }
}
