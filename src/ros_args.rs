//! Parsing helpers for the ROS 2 `--ros-args` CLI convention.
//!
//! `ros2 run <pkg> <exe>` and a launch `Node(...)` append a trailing
//! `--ros-args … [--]` block (remaps, params, log config, enclave) *after* the
//! program's own flags. `asset_server` parses its own flags with `clap`, which
//! errors on those tokens — so `main` hands clap only the non-ROS argv (via
//! [`strip_ros_args`]).
//!
//! r2r's `Context::create()` already feeds the **full** process argv to
//! `rcl_init` (node options default `use_global_arguments = true`), so remaps
//! reach rcl. But r2r's `Node::create` takes an **explicit** node name +
//! namespace, so `main` mirrors the launch `__node` / `__ns` remaps into them
//! via [`ros_node_remap`] / [`ros_namespace_remap`] (a launch `name=` /
//! `namespace=` then wins over the `--node-name` / `--namespace` flags + env).
//!
//! Pure string logic, no ROS deps — unit-tested on the host.

/// Split `argv` into `(kept, ros_args)`.
///
/// Every token inside a `--ros-args … [--]` region goes to `ros_args` (the
/// `--ros-args` marker and the region-terminating `--` are consumed, not kept);
/// everything else — including `argv[0]` and any normal `--` end-of-options
/// marker outside a ROS region — is kept for the program's own parser. Multiple
/// `--ros-args` regions are supported (ros2 can emit more than one).
pub fn strip_ros_args(argv: impl IntoIterator<Item = String>) -> (Vec<String>, Vec<String>) {
    let mut kept = Vec::new();
    let mut ros = Vec::new();
    let mut it = argv.into_iter();
    while let Some(tok) = it.next() {
        if tok == "--ros-args" {
            // A ROS-args region runs to the next `--` (exclusive) or end of argv.
            for t in it.by_ref() {
                if t == "--" {
                    break;
                }
                ros.push(t);
            }
        } else {
            kept.push(tok);
        }
    }
    (kept, ros)
}

/// Extract the ROS namespace remap (`__ns`) from a `--ros-args` token slice.
pub fn ros_namespace_remap(ros_args: &[String]) -> Option<String> {
    ros_remap_target(ros_args, "__ns")
}

/// Extract the ROS node-name remap (`__node`) from a `--ros-args` token slice.
pub fn ros_node_remap(ros_args: &[String]) -> Option<String> {
    ros_remap_target(ros_args, "__node")
}

/// Return the value of the last `<key>:=<value>` remap rule for `key`, accepting
/// `-r`/`--remap`-introduced rules or a bare `key:=value` token (last wins,
/// matching rcl's last-rule-wins semantics).
fn ros_remap_target(ros_args: &[String], key: &str) -> Option<String> {
    let prefix = format!("{key}:=");
    let mut found = None;
    let mut i = 0;
    while i < ros_args.len() {
        let rule = if ros_args[i] == "-r" || ros_args[i] == "--remap" {
            i += 1;
            ros_args.get(i)
        } else {
            Some(&ros_args[i])
        };
        if let Some(v) = rule.and_then(|r| r.strip_prefix(&prefix)) {
            found = Some(v.to_owned());
        }
        i += 1;
    }
    found
}

#[cfg(test)]
mod tests {
    use super::*;

    fn v(args: &[&str]) -> Vec<String> {
        args.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn strips_trailing_ros_args_block() {
        let (kept, ros) = strip_ros_args(v(&[
            "asset_server",
            "--heartbeat",
            "2.0",
            "--ros-args",
            "-r",
            "__ns:=/rosbot",
            "-r",
            "__node:=assets",
        ]));
        assert_eq!(kept, v(&["asset_server", "--heartbeat", "2.0"]));
        assert_eq!(ros, v(&["-r", "__ns:=/rosbot", "-r", "__node:=assets"]));
    }

    #[test]
    fn ros_args_region_ends_at_double_dash_and_resumes() {
        let (kept, ros) = strip_ros_args(v(&[
            "prog",
            "--ros-args",
            "-r",
            "__ns:=/a",
            "--",
            "--heartbeat",
            "3.0",
        ]));
        assert_eq!(kept, v(&["prog", "--heartbeat", "3.0"]));
        assert_eq!(ros, v(&["-r", "__ns:=/a"]));
    }

    #[test]
    fn keeps_a_plain_end_of_options_marker_outside_a_ros_region() {
        let (kept, ros) = strip_ros_args(v(&["prog", "--", "positional"]));
        assert_eq!(kept, v(&["prog", "--", "positional"]));
        assert!(ros.is_empty());
    }

    #[test]
    fn no_ros_args_is_identity() {
        let (kept, ros) = strip_ros_args(v(&["prog", "--heartbeat", "5"]));
        assert_eq!(kept, v(&["prog", "--heartbeat", "5"]));
        assert!(ros.is_empty());
    }

    #[test]
    fn namespace_and_node_remap_dashr_long_and_bare() {
        assert_eq!(
            ros_namespace_remap(&v(&["-r", "__ns:=/rosbot"])),
            Some("/rosbot".into())
        );
        assert_eq!(
            ros_namespace_remap(&v(&["--remap", "__ns:=/lynx"])),
            Some("/lynx".into())
        );
        assert_eq!(ros_namespace_remap(&v(&["__ns:=/bare"])), Some("/bare".into()));
        assert_eq!(
            ros_node_remap(&v(&["-r", "__node:=assets"])),
            Some("assets".into())
        );
    }

    #[test]
    fn last_remap_wins_and_ignores_other_rules() {
        assert_eq!(
            ros_namespace_remap(&v(&[
                "-r",
                "__node:=assets",
                "-r",
                "__ns:=/first",
                "-r",
                "/foo:=/bar",
                "-r",
                "__ns:=/second",
            ])),
            Some("/second".into())
        );
    }

    #[test]
    fn no_remap_returns_none() {
        assert_eq!(ros_namespace_remap(&v(&["-r", "__node:=x"])), None);
        assert_eq!(ros_node_remap(&v(&["-r", "__ns:=/x"])), None);
        assert_eq!(ros_namespace_remap(&[]), None);
    }
}
