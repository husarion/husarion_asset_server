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

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use clap::Parser;
use futures::executor::LocalPool;
use futures::task::LocalSpawnExt;
use r2r::QosProfile;
use serde_json::{json, Value};

const GET_ASSET_TYPE: &str = "husarion_asset_msgs/srv/GetAsset";

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
}

struct Check {
    name: String,
    ok: bool,
    detail: String,
}

fn main() -> anyhow::Result<()> {
    let args = Args::parse();
    let ctx = r2r::Context::create()?;
    let mut node = r2r::Node::create(ctx, "husarion_asset_conformance", "")?;
    let client =
        node.create_client_untyped(&args.service, GET_ASSET_TYPE, QosProfile::default())?;
    // Wait for the provider's service to be discovered before requesting (a fresh
    // process needs DDS discovery to settle, especially across containers).
    let available = r2r::Node::is_available(&client)?;

    let results: Arc<Mutex<Vec<Check>>> = Arc::new(Mutex::new(Vec::new()));
    let done = Arc::new(AtomicBool::new(false));

    let pool = LocalPool::new();
    let spawner = pool.spawner();
    {
        let results = Arc::clone(&results);
        let done = Arc::clone(&done);
        let uri = args.uri.clone();
        let unknown = args.unknown_package.clone();
        spawner.spawn_local(async move {
            if available.await.is_err() {
                results.lock().unwrap().push(Check {
                    name: "service available".into(),
                    ok: false,
                    detail: "service never became available".into(),
                });
                done.store(true, Ordering::SeqCst);
                return;
            }
            run_checks(&client, &uri, &unknown, &results).await;
            done.store(true, Ordering::SeqCst);
        })?;
    }

    let mut pool = pool;
    let started = std::time::Instant::now();
    loop {
        node.spin_once(Duration::from_millis(20));
        pool.run_until_stalled();
        if done.load(Ordering::SeqCst) {
            break;
        }
        if started.elapsed() > Duration::from_secs(30) {
            results.lock().unwrap().push(Check {
                name: "completed within 30s".into(),
                ok: false,
                detail: "timed out (is the provider up?)".into(),
            });
            break;
        }
    }

    let checks = results.lock().unwrap();
    let mut failed = 0;
    println!("\nhusarion_asset conformance — service {}\n", args.service);
    for c in checks.iter() {
        let tag = if c.ok {
            "\x1b[32mPASS\x1b[0m"
        } else {
            "\x1b[31mFAIL\x1b[0m"
        };
        let suffix = if c.detail.is_empty() {
            String::new()
        } else {
            format!(" — {}", c.detail)
        };
        println!("  {tag}  {}{suffix}", c.name);
        if !c.ok {
            failed += 1;
        }
    }
    println!("\n{}/{} checks passed", checks.len() - failed, checks.len());
    std::process::exit(if failed == 0 { 0 } else { 1 });
}

async fn run_checks(
    client: &r2r::ClientUntyped,
    uri: &str,
    unknown_package: &str,
    out: &Arc<Mutex<Vec<Check>>>,
) {
    let push = |name: &str, ok: bool, detail: String| {
        out.lock().unwrap().push(Check {
            name: name.to_string(),
            ok,
            detail,
        });
    };

    // 1. Full fetch.
    let full = match call(client, json!({"uri": uri, "offset": 0, "length": 0})).await {
        Ok(v) => v,
        Err(e) => {
            push("full fetch", false, format!("request failed: {e}"));
            return;
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
        .unwrap_or(0);
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
        data_len as u64 == total,
        format!("data={data_len} total={total}"),
    );

    // 2. Chunked fetch stitches to the whole + stable hash.
    if total > 1 {
        let half = total / 2;
        let a = call(client, json!({"uri": uri, "offset": 0, "length": half})).await;
        let b = call(client, json!({"uri": uri, "offset": half, "length": 0})).await;
        match (a, b) {
            (Ok(a), Ok(b)) => {
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
            }
            _ => push("chunked fetch", false, "a chunk request failed".into()),
        }
    }

    // 3. Hash determinism.
    if let Ok(again) = call(client, json!({"uri": uri, "offset": 0, "length": 0})).await {
        let h2 = again
            .get("content_hash")
            .and_then(Value::as_str)
            .unwrap_or("");
        push("content_hash is deterministic", h2 == hash, String::new());
    }

    // 4. Unknown package rejected.
    let bad_uri = format!("package://{unknown_package}/x.dae");
    if let Ok(v) = call(client, json!({"uri": bad_uri, "offset": 0, "length": 0})).await {
        let s = v.get("success").and_then(Value::as_bool).unwrap_or(true);
        push(
            "unknown package rejected (success:false)",
            !s,
            String::new(),
        );
    }

    // 5. Path traversal rejected — take the package, append /../etc/passwd.
    let pkg = uri
        .strip_prefix("package://")
        .and_then(|r| r.split('/').next())
        .unwrap_or("");
    let traversal = format!("package://{pkg}/../etc/passwd");
    if let Ok(v) = call(client, json!({"uri": traversal, "offset": 0, "length": 0})).await {
        let s = v.get("success").and_then(Value::as_bool).unwrap_or(true);
        push("path traversal rejected (success:false)", !s, String::new());
    }
}

/// One untyped GetAsset call.
async fn call(client: &r2r::ClientUntyped, req: Value) -> Result<Value, String> {
    client
        .request(req)
        .map_err(|e| e.to_string())?
        .await
        .map_err(|_| "client cancelled".to_string())?
        .map_err(|e| e.to_string())
}
