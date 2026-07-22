//! ROS 2 parameter interface for `asset_server`.
//!
//! Every operator knob the node accepts as a CLI flag and/or an `ASSET_SERVER_*`
//! env var is *also* a ROS 2 parameter, so the provider is configured the
//! idiomatic ROS way — a launch `parameters=[{…}]` block, a params YAML, or
//! `-p k:=v` — and introspected with `ros2 param list/get/describe`.
//!
//! **Precedence: CLI flag > ROS param > env var > built-in default.** This falls
//! out of one trick (see `main.rs`): a set ROS param is written into its env var
//! *before* the CLI is parsed, so clap treats it as the env value while an
//! explicit flag still overrides it — flag > param > env > default without
//! special-casing every knob.
//!
//! The node name and namespace are NOT knobs here — they are the ROS node
//! identity, set the standard way (`-r __node:=…` / `-r __ns:=…`, or a launch
//! `name=` / `namespace=`), handled in `main.rs` via `ros_args`.
//!
//! [`KNOBS`] is the single source of truth: the overlay, the effective values
//! published back for `ros2 param get`, and the docs all read from it.

use r2r::ParameterValue;

/// How a knob is represented as an env-var string (the wire the overlay rides)
/// and as a ROS [`ParameterValue`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    /// A scalar string (a topic name).
    Str,
    /// A non-negative integer (a byte ceiling).
    Int,
    /// A floating-point scalar (a period in seconds).
    Float,
    /// A string list serialized to a **comma**-separated env var (matches clap's
    /// `value_delimiter = ','` on `owned_packages`).
    ListComma,
}

/// One operator knob: its ROS parameter name, backing env var, type, default (as
/// the env string), and one-line help (surfaced by `ros2 param describe`).
#[derive(Clone, Copy, Debug)]
pub struct Knob {
    pub param: &'static str,
    pub env: &'static str,
    pub kind: Kind,
    pub default: &'static str,
    pub help: &'static str,
}

/// Every operator knob, keyed by ROS parameter name.
pub const KNOBS: &[Knob] = &[
    Knob {
        param: "owned_packages",
        env: "ASSET_SERVER_OWNED_PACKAGES",
        kind: Kind::ListComma,
        default: "",
        help: "Explicit owned package names; empty = auto-derive from the description topic",
    },
    Knob {
        param: "description_topic",
        env: "ASSET_SERVER_DESCRIPTION_TOPIC",
        kind: Kind::Str,
        default: "robot_description",
        help: "Latched URDF/description topic to auto-derive owned packages from",
    },
    Knob {
        param: "providers_topic",
        env: "ASSET_SERVER_PROVIDERS_TOPIC",
        kind: Kind::Str,
        default: "/asset_providers",
        help: "Topic the provider announces AssetProviderInfo on (latched)",
    },
    Knob {
        param: "heartbeat",
        env: "ASSET_SERVER_HEARTBEAT",
        kind: Kind::Float,
        default: "5.0",
        help: "Re-announce / heartbeat period, seconds",
    },
    Knob {
        param: "max_chunk",
        env: "ASSET_SERVER_MAX_CHUNK",
        kind: Kind::Int,
        default: "524288",
        help: "GetAsset response chunk ceiling, bytes",
    },
];

/// Look up a knob by ROS parameter name.
pub fn knob(param: &str) -> Option<&'static Knob> {
    KNOBS.iter().find(|k| k.param == param)
}

/// Serialize a ROS [`ParameterValue`] to the env-var string for a knob.
/// `Ok(None)` = the value is `NotSet` / an empty list (skip). `Err` = a type
/// mismatch to warn on.
pub fn value_to_env(kind: Kind, v: &ParameterValue) -> Result<Option<String>, String> {
    let mismatch = |want: &str, got: &ParameterValue| Err(format!("expected {want}, got {got:?}"));
    let s = match (kind, v) {
        (_, ParameterValue::NotSet) => return Ok(None),
        (Kind::Str, ParameterValue::String(s)) => s.clone(),
        (Kind::Str, _) => return mismatch("a string", v),
        (Kind::Int, ParameterValue::Integer(i)) => i.to_string(),
        (Kind::Int, _) => return mismatch("an integer", v),
        // Accept an int for a float knob (a YAML `5` instead of `5.0`).
        (Kind::Float, ParameterValue::Double(d)) => d.to_string(),
        (Kind::Float, ParameterValue::Integer(i)) => i.to_string(),
        (Kind::Float, _) => return mismatch("a double", v),
        // An empty list is a no-op (skip) rather than a stray empty element.
        (Kind::ListComma, ParameterValue::StringArray(a)) if a.is_empty() => return Ok(None),
        (Kind::ListComma, ParameterValue::StringArray(a)) => a.join(","),
        // A single string for a list knob is accepted (one package).
        (Kind::ListComma, ParameterValue::String(s)) => s.clone(),
        (Kind::ListComma, _) => return mismatch("a string array", v),
    };
    Ok(Some(s))
}

/// Build a ROS [`ParameterValue`] from a knob's effective env-string, for
/// publishing effective values back so `ros2 param get` is accurate.
pub fn env_to_value(kind: Kind, s: &str) -> ParameterValue {
    match kind {
        Kind::Str => ParameterValue::String(s.to_owned()),
        Kind::Int => s
            .parse::<i64>()
            .map(ParameterValue::Integer)
            .unwrap_or(ParameterValue::NotSet),
        Kind::Float => s
            .parse::<f64>()
            .map(ParameterValue::Double)
            .unwrap_or(ParameterValue::NotSet),
        Kind::ListComma => ParameterValue::StringArray(
            s.split(',')
                .filter(|t| !t.is_empty())
                .map(str::to_owned)
                .collect(),
        ),
    }
}
