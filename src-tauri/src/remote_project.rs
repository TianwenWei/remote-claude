//! File and Git operations for registered SSH workspaces. All user input is
//! JSON/base64 data; the remote Python helper validates real paths before I/O.
use base64::Engine as _;
use serde::de::DeserializeOwned;
use serde_json::{json, Value};

const HELPER: &str = include_str!("remote_project/operations.py");

fn is_read_only_operation(op: &str) -> bool {
    matches!(
        op,
        "list_dir"
            | "read_file"
            | "search"
            | "index"
            | "git_tree"
            | "git_summary"
            | "git_status"
            | "git_diff"
            | "git_branches"
    )
}

pub(crate) fn script(root: &str, path: &str, op: &str, args: Value) -> String {
    let request = json!({"root": root, "path": path, "op": op, "args": args});
    let payload = base64::engine::general_purpose::STANDARD.encode(request.to_string());
    format!("python3 - <<'CCGUI_REMOTE_PROJECT_PY'\nREQUEST_B64 = '{payload}'\n{HELPER}\nCCGUI_REMOTE_PROJECT_PY\n")
}

pub(crate) fn call<T: DeserializeOwned>(
    db: &crate::db::Db,
    path: &str,
    op: &str,
    args: Value,
) -> Result<Option<T>, String> {
    use crate::engine::wsl_transport::{
        run_readonly_script_output_blocking, run_script_output_blocking, transport_for_path,
    };
    let Some(transport) = transport_for_path(db, path)? else {
        return Ok(None);
    };
    let root = transport
        .workspace
        .as_deref()
        .ok_or("remote workspace root is missing")?;
    let body = script(root, path, op, args);
    // Only an explicit read allowlist may reuse a persistent SSH connection.
    // Cancellation of a mutating RPC must retain its dedicated connection.
    let output = if is_read_only_operation(op) {
        run_readonly_script_output_blocking(&transport, &body)
    } else {
        run_script_output_blocking(&transport, &body)
    }?;
    let response: Value = serde_json::from_str(output.trim())
        .map_err(|e| format!("invalid remote project response: {e}"))?;
    if let Some(error) = response.get("error").and_then(Value::as_str) {
        return Err(error.to_string());
    }
    let value = response
        .get("result")
        .ok_or("remote project response is missing result")?;
    serde_json::from_value(value.clone())
        .map(Some)
        .map_err(|e| e.to_string())
}

pub(crate) fn reject_cross_remote_target(db: &crate::db::Db, path: &str) -> Result<(), String> {
    if crate::engine::wsl_transport::transport_for_path(db, path)?.is_some() {
        return Err(
            "copying or moving between local and SSH workspaces is not supported".to_string(),
        );
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::process::Command;

    #[test]
    fn ssh_reuse_is_opt_in_and_every_mutating_operation_stays_dedicated() {
        for op in [
            "list_dir",
            "read_file",
            "search",
            "index",
            "git_tree",
            "git_summary",
            "git_status",
            "git_diff",
            "git_branches",
        ] {
            assert!(is_read_only_operation(op), "read operation {op}");
        }
        for op in [
            "write_file",
            "create_file",
            "create_dir",
            "rename",
            "trash",
            "duplicate",
            "paste",
            "git_stage",
            "git_unstage",
            "git_discard",
            "git_commit",
            "git_push",
            "git_pull",
            "git_checkout",
            "git_create_branch",
            "future_unknown_operation",
        ] {
            assert!(!is_read_only_operation(op), "unsafe reuse for {op}");
        }
    }

    fn invoke(root: &str, path: &str, op: &str, args: Value) -> Value {
        let output = Command::new("bash")
            .arg("-c")
            .arg(script(root, path, op, args))
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        serde_json::from_slice(&output.stdout).unwrap()
    }

    struct Scratch(std::path::PathBuf);
    impl Scratch {
        fn new() -> Self {
            let path = std::env::temp_dir().join(format!("ccgui-ssh-rpc-{}", uuid::Uuid::new_v4()));
            std::fs::create_dir_all(&path).unwrap();
            Self(path)
        }
        fn path(&self) -> String {
            self.0.to_string_lossy().into_owned()
        }
    }
    impl Drop for Scratch {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn remote_rpc_preserves_unicode_and_shell_metacharacters() {
        let scratch = Scratch::new();
        let file = scratch
            .0
            .join("中文 ' $(touch INJECTION) `id`.txt")
            .to_string_lossy()
            .into_owned();
        let content = "hello\n中文🙂 ' $(id) `whoami`\n";
        assert_eq!(
            invoke(
                &scratch.path(),
                &file,
                "write_file",
                json!({"content": content})
            )["result"],
            Value::Null
        );
        assert_eq!(
            invoke(&scratch.path(), &file, "read_file", json!({}))["result"]["text"],
            content
        );
        assert!(!scratch.0.join("INJECTION").exists());
    }

    #[test]
    fn remote_rpc_rejects_parent_and_symlink_escape() {
        let scratch = Scratch::new();
        let outside = Scratch::new();
        let escaped = format!("{}/../outside.txt", scratch.path());
        assert!(invoke(
            &scratch.path(),
            &escaped,
            "write_file",
            json!({"content": "bad"})
        )["error"]
            .is_string());
        #[cfg(unix)]
        {
            std::os::unix::fs::symlink(&outside.0, scratch.0.join("escape")).unwrap();
            let escaped = scratch
                .0
                .join("escape/new.txt")
                .to_string_lossy()
                .into_owned();
            assert!(invoke(
                &scratch.path(),
                &escaped,
                "write_file",
                json!({"content": "bad"})
            )["error"]
                .is_string());
            assert!(!outside.0.join("new.txt").exists());
        }
    }

    #[test]
    fn remote_rpc_git_status_diff_and_stage_use_remote_repository() {
        let scratch = Scratch::new();
        let status = Command::new("git")
            .args(["init", "-q"])
            .arg(&scratch.0)
            .status()
            .unwrap();
        assert!(status.success());
        std::fs::write(scratch.0.join("new file.txt"), "one\ntwo\n").unwrap();
        let status = invoke(&scratch.path(), &scratch.path(), "git_status", json!({}));
        assert_eq!(status["result"]["untracked"][0]["path"], "new file.txt");
        let diff = invoke(
            &scratch.path(),
            &scratch.path(),
            "git_diff",
            json!({"file": "new file.txt", "staged": false}),
        );
        assert!(diff["result"].as_str().unwrap().contains("+two"));
        assert!(invoke(
            &scratch.path(),
            &scratch.path(),
            "git_stage",
            json!({"files": ["new file.txt"]})
        )
        .get("error")
        .is_none());
        let status = invoke(&scratch.path(), &scratch.path(), "git_status", json!({}));
        assert_eq!(status["result"]["staged"][0]["path"], "new file.txt");
        assert_eq!(status["result"]["staged"][0]["additions"], 2);
        assert!(invoke(
            &scratch.path(),
            &scratch.path(),
            "git_unstage",
            json!({"files": ["new file.txt"]})
        )
        .get("error")
        .is_none());
        assert!(scratch.0.join("new file.txt").exists());
        assert!(invoke(
            &scratch.path(),
            &scratch.path(),
            "git_stage",
            json!({"files": ["../outside.txt"]})
        )["error"]
            .is_string());
    }
}
