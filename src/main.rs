//! husarion_asset_server — reference provider for the husarion_asset_msgs standard.
//!
//! Serves a component's `package://` assets (meshes, textures, URDF resources) over
//! the ROS graph and announces which packages it owns, so a bridge / client can
//! fetch them without baking meshes into any image.
//!
//! Because a provider hosts exactly one known service (`GetAsset`), it uses r2r's
//! **typed** service server — no untyped-server FFI needed. r2r generates the
//! `GetAsset` / `AssetProviderInfo` bindings from a sourced `husarion_asset_msgs`.
//!
//! Behaviour (see the husarion_asset_msgs README — the normative spec):
//! ranged `GetAsset` with `total_size` + `content_hash` for chunking + caching;
//! latched `AssetProviderInfo` announce + heartbeat; owned-package set explicit or
//! auto-derived from a co-located `robot_description`. Security: `package://` only,
//! no `..` traversal, owned-set only, resolved real path confined to the package
//! share dir.

use std::collections::{HashMap, HashSet};
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, UNIX_EPOCH};

use clap::Parser;
use futures::executor::LocalPool;
use futures::stream::StreamExt;
use futures::task::LocalSpawnExt;
use r2r::builtin_interfaces::msg::Time;
use r2r::husarion_asset_msgs::msg::AssetProviderInfo;
use r2r::husarion_asset_msgs::srv::GetAsset;
use r2r::std_msgs::msg::String as RosString;
use r2r::QosProfile;
use sha2::{Digest, Sha256};

/// Default response chunk ceiling — comfortably under common RMW service limits.
const DEFAULT_MAX_CHUNK: usize = 512 * 1024;

#[derive(Parser, Debug)]
#[command(
    name = "asset_server",
    about = "Reference package:// asset provider (r2r)"
)]
struct Args {
    /// ROS node name (must be unique per provider on the graph).
    #[arg(
        long,
        env = "ASSET_SERVER_NODE",
        default_value = "husarion_asset_server"
    )]
    node_name: String,
    /// ROS namespace (empty = global).
    #[arg(long, env = "ROS_NAMESPACE", default_value = "")]
    namespace: String,
    /// Explicit owned packages. When empty, derive from the description topic.
    #[arg(long, value_delimiter = ',')]
    owned_packages: Vec<String>,
    /// Latched URDF source for auto-derivation.
    #[arg(long, default_value = "robot_description")]
    description_topic: String,
    /// Where AssetProviderInfo is announced.
    #[arg(long, default_value = "/asset_providers")]
    providers_topic: String,
    /// Re-announce period (seconds).
    #[arg(long, default_value_t = 5.0)]
    heartbeat: f64,
    /// Response chunk ceiling (bytes).
    #[arg(long, default_value_t = DEFAULT_MAX_CHUNK)]
    max_chunk: usize,
}

fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()),
        )
        .init();

    let args = Args::parse();
    let ctx = r2r::Context::create()?;
    let mut node = r2r::Node::create(ctx, &args.node_name, &args.namespace)?;

    let owned: Arc<Mutex<HashSet<String>>> =
        Arc::new(Mutex::new(args.owned_packages.iter().cloned().collect()));
    let auto = owned.lock().unwrap().is_empty();

    let node_fqn = node.fully_qualified_name()?;
    let service_name = format!("{}/get_asset", node_fqn);
    let service = node.create_service::<GetAsset::Service>(&service_name, QosProfile::default())?;
    let announce_pub = node.create_publisher::<AssetProviderInfo>(
        &args.providers_topic,
        QosProfile::default().transient_local(),
    )?;
    let clock = node.get_ros_clock();
    let mut timer = node.create_wall_timer(Duration::from_secs_f64(args.heartbeat))?;
    let desc_sub = if auto {
        Some(node.subscribe::<RosString>(
            &args.description_topic,
            QosProfile::default().transient_local(),
        )?)
    } else {
        tracing::info!(packages = ?owned.lock().unwrap(), "serving declared packages");
        None
    };

    let pool = LocalPool::new();
    let spawner = pool.spawner();

    // GetAsset handler.
    {
        let owned = Arc::clone(&owned);
        let max_chunk = args.max_chunk;
        let mut service = service;
        spawner.spawn_local(async move {
            let mut cache: HashMap<String, (u64, u128, String)> = HashMap::new();
            while let Some(req) = service.next().await {
                let resp = {
                    let set = owned.lock().unwrap();
                    handle_get_asset(&req.message, &set, max_chunk, &mut cache)
                };
                let _ = req.respond(resp);
            }
        })?;
    }

    // Auto-derive ownership from the latched robot_description.
    if let Some(mut sub) = desc_sub {
        let owned = Arc::clone(&owned);
        spawner.spawn_local(async move {
            while let Some(msg) = sub.next().await {
                let pkgs = packages_from_urdf(&msg.data);
                if !pkgs.is_empty() {
                    let mut set = owned.lock().unwrap();
                    if *set != pkgs {
                        tracing::info!(?pkgs, "owned packages (from description)");
                        *set = pkgs;
                    }
                }
            }
        })?;
    }

    // Latched announce + heartbeat.
    {
        let owned = Arc::clone(&owned);
        let hb = args.heartbeat as f32;
        let svc = service_name.clone();
        let fqn = node_fqn.clone();
        spawner.spawn_local(async move {
            loop {
                let stamp = now(&clock);
                let packages = {
                    let mut v: Vec<String> = owned.lock().unwrap().iter().cloned().collect();
                    v.sort();
                    v
                };
                let info = AssetProviderInfo {
                    node_name: fqn.clone(),
                    service_name: svc.clone(),
                    packages,
                    stamp,
                    heartbeat_period_sec: hb,
                };
                let _ = announce_pub.publish(&info);
                if timer.tick().await.is_err() {
                    break;
                }
            }
        })?;
    }

    tracing::info!(service = %service_name, "husarion_asset_server ready");
    let mut pool = pool;
    loop {
        node.spin_once(Duration::from_millis(100));
        pool.run_until_stalled();
    }
}

fn now(clock: &Arc<Mutex<r2r::Clock>>) -> Time {
    let d = clock.lock().unwrap().get_now().unwrap_or_default();
    r2r::Clock::to_builtin_time(&d)
}

fn handle_get_asset(
    req: &GetAsset::Request,
    owned: &HashSet<String>,
    max_chunk: usize,
    cache: &mut HashMap<String, (u64, u128, String)>,
) -> GetAsset::Response {
    let mut resp = GetAsset::Response {
        success: false,
        error: String::new(),
        media_type: String::new(),
        total_size: 0,
        content_hash: String::new(),
        data: Vec::new(),
    };

    let path = match resolve_uri(&req.uri, owned) {
        Ok(p) => p,
        Err(e) => {
            resp.error = e;
            return resp;
        }
    };

    let meta = match std::fs::metadata(&path) {
        Ok(m) => m,
        Err(e) => {
            resp.error = format!("stat failed: {e}");
            return resp;
        }
    };
    let total = meta.len();
    let content_hash = match content_hash(&path, &meta, cache) {
        Ok(h) => h,
        Err(e) => {
            resp.error = format!("hash failed: {e}");
            return resp;
        }
    };

    let offset = req.offset;
    if offset > total {
        resp.error = format!("offset {offset} past end ({total})");
        return resp;
    }
    let want = if req.length == 0 {
        total - offset
    } else {
        u64::from(req.length)
    };
    let length = want.min(max_chunk as u64).min(total - offset);

    let chunk = match read_range(&path, offset, length as usize) {
        Ok(c) => c,
        Err(e) => {
            resp.error = format!("read failed: {e}");
            return resp;
        }
    };

    resp.success = true;
    resp.media_type = media_type(&path).to_string();
    resp.total_size = total;
    resp.content_hash = content_hash;
    resp.data = chunk;
    resp
}

/// Resolve `package://PKG/REL` to a confined filesystem path, enforcing the
/// provider security rules.
fn resolve_uri(uri: &str, owned: &HashSet<String>) -> Result<PathBuf, String> {
    let rest = uri
        .strip_prefix("package://")
        .ok_or_else(|| "only package:// URIs are served".to_string())?;
    let (pkg, rel) = rest
        .split_once('/')
        .ok_or_else(|| format!("malformed package URI: {uri}"))?;
    if pkg.is_empty() || rel.is_empty() {
        return Err(format!("malformed package URI: {uri}"));
    }
    if !owned.contains(pkg) {
        return Err(format!("package not owned by this provider: {pkg}"));
    }
    if rel.split('/').any(|seg| seg == "..") {
        return Err("path traversal rejected".to_string());
    }
    let share = package_share_dir(pkg)
        .ok_or_else(|| format!("package not found on AMENT_PREFIX_PATH: {pkg}"))?;
    let share_real = share
        .canonicalize()
        .map_err(|e| format!("share canonicalize: {e}"))?;
    let candidate = share_real.join(rel);
    let candidate = candidate
        .canonicalize()
        .map_err(|_| format!("not a file: {uri}"))?;
    if !candidate.starts_with(&share_real) {
        return Err("resolved path escapes the package share dir".to_string());
    }
    if !candidate.is_file() {
        return Err(format!("not a file: {uri}"));
    }
    Ok(candidate)
}

/// The ament-index resolution: the first `<prefix>/share/<pkg>` that exists on
/// `AMENT_PREFIX_PATH`.
fn package_share_dir(pkg: &str) -> Option<PathBuf> {
    let ament = std::env::var("AMENT_PREFIX_PATH").ok()?;
    for prefix in ament.split(':').filter(|p| !p.is_empty()) {
        let cand = Path::new(prefix).join("share").join(pkg);
        if cand.is_dir() {
            return Some(cand);
        }
    }
    None
}

fn read_range(path: &Path, offset: u64, length: usize) -> std::io::Result<Vec<u8>> {
    let mut f = std::fs::File::open(path)?;
    f.seek(SeekFrom::Start(offset))?;
    let mut buf = vec![0u8; length];
    f.read_exact(&mut buf)?;
    Ok(buf)
}

fn content_hash(
    path: &Path,
    meta: &std::fs::Metadata,
    cache: &mut HashMap<String, (u64, u128, String)>,
) -> std::io::Result<String> {
    let size = meta.len();
    let mtime = meta
        .modified()
        .ok()
        .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let key = path.to_string_lossy().into_owned();
    if let Some((s, m, h)) = cache.get(&key) {
        if *s == size && *m == mtime {
            return Ok(h.clone());
        }
    }
    let mut f = std::fs::File::open(path)?;
    let mut hasher = Sha256::new();
    let mut buf = [0u8; 1 << 16];
    loop {
        let n = f.read(&mut buf)?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
    }
    let digest = format!("sha256:{:x}", hasher.finalize());
    cache.insert(key, (size, mtime, digest.clone()));
    Ok(digest)
}

fn media_type(path: &Path) -> &'static str {
    match path
        .extension()
        .and_then(|e| e.to_str())
        .map(str::to_ascii_lowercase)
        .as_deref()
    {
        Some("dae") => "model/vnd.collada+xml",
        Some("stl") => "model/stl",
        Some("obj") => "model/obj",
        Some("mtl") => "model/mtl",
        Some("urdf" | "xacro" | "xml") => "application/xml",
        Some("png") => "image/png",
        Some("jpg" | "jpeg") => "image/jpeg",
        Some("gltf") => "model/gltf+json",
        Some("glb") => "model/gltf-binary",
        _ => "application/octet-stream",
    }
}

/// Scrape every `package://PKG/...` referenced by a URDF/xacro string.
fn packages_from_urdf(urdf: &str) -> HashSet<String> {
    let mut set = HashSet::new();
    for (i, _) in urdf.match_indices("package://") {
        let rest = &urdf[i + "package://".len()..];
        let pkg: String = rest
            .chars()
            .take_while(|c| !matches!(c, '/' | '"' | '\'' | '<' | '>') && !c.is_whitespace())
            .collect();
        if !pkg.is_empty() {
            set.insert(pkg);
        }
    }
    set
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scrapes_urdf_packages() {
        let urdf = r#"<robot><link><visual><geometry>
            <mesh filename="package://rosbot_description/meshes/body.dae"/>
            </geometry></visual></link>
            <link><visual><geometry>
            <mesh filename="package://husarion_components_description/meshes/x.stl"/>
            </geometry></visual></link></robot>"#;
        let pkgs = packages_from_urdf(urdf);
        assert!(pkgs.contains("rosbot_description"));
        assert!(pkgs.contains("husarion_components_description"));
        assert_eq!(pkgs.len(), 2);
    }

    #[test]
    fn media_types() {
        assert_eq!(media_type(Path::new("a/b.dae")), "model/vnd.collada+xml");
        assert_eq!(media_type(Path::new("a/b.STL")), "model/stl");
        assert_eq!(media_type(Path::new("a/b.bin")), "application/octet-stream");
    }
}
