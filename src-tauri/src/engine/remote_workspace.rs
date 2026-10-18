//! Native Linux SSH workspaces. Connection details are stored locally; the
//! remote login shell owns Claude's binary, credentials and configuration.
use super::wsl_transport::{self, WslTransport};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::{LazyLock, Mutex};
use std::time::{Duration, Instant};

type HistoryCache = HashMap<String, (Instant, usize, Vec<crate::history::SessionMeta>)>;
static HISTORY_CACHE: LazyLock<Mutex<HistoryCache>> = LazyLock::new(|| Mutex::new(HashMap::new()));

#[derive(Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SshConnectionInput {
    pub host: String,
    pub user: String,
    #[serde(default = "default_port")]
    pub port: u16,
    pub identity_file: Option<String>,
}
fn default_port() -> u16 {
    22
}

#[derive(Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RemoteDirectory {
    pub name: String,
    pub path: String,
}

#[derive(Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RemoteDependency {
    pub available: bool,
    pub version: Option<String>,
}

#[derive(Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RemoteDependencies {
    pub bash: RemoteDependency,
    pub python: RemoteDependency,
    pub git: RemoteDependency,
    pub claude: RemoteDependency,
}

#[derive(Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ClaudeAuthStatus {
    pub status: String,
    pub method: Option<String>,
}

#[derive(Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SshConnectionProbe {
    pub home: String,
    pub claude_available: bool,
    pub claude_version: Option<String>,
    pub canonical_path: String,
    pub directories: Vec<RemoteDirectory>,
    pub dependencies: RemoteDependencies,
    pub claude_auth: ClaudeAuthStatus,
    #[serde(default, skip_serializing)]
    pub claude_path: Option<String>,
}

fn transport(connection: &SshConnectionInput, workspace: &str) -> Result<WslTransport, String> {
    let meta = serde_json::json!({"ssh": {
        "host":connection.host.trim(), "user":connection.user.trim(), "port":connection.port,
        "identityFile":connection.identity_file, "workspace":workspace
    }});
    let tp = wsl_transport::from_workspace_meta_checked(&meta)?.ok_or("Missing SSH connection")?;
    if let Some(key) = &tp.identity_file {
        if !std::path::Path::new(key).is_file() {
            return Err("SSH identity file does not exist".into());
        }
    }
    Ok(tp)
}

async fn probe(
    connection: &SshConnectionInput,
    path: Option<&str>,
) -> Result<SshConnectionProbe, String> {
    let tp = transport(connection, "/")?;
    let path = path.filter(|path| !path.trim().is_empty()).unwrap_or("~");
    if path.contains(['\0', '\n', '\r']) {
        return Err("Invalid remote directory".into());
    }
    let output = wsl_transport::run_readonly_probe_output(&tp, &probe_script(path)).await?;
    serde_json::from_str(&output).map_err(|e| format!("Invalid SSH probe response: {e}"))
}

#[tauri::command]
pub async fn probe_ssh_connection(
    connection: SshConnectionInput,
    path: Option<String>,
) -> Result<SshConnectionProbe, String> {
    probe(&connection, path.as_deref()).await
}

#[tauri::command]
pub async fn add_ssh_workspace(
    state: tauri::State<'_, crate::AppState>,
    connection: SshConnectionInput,
    path: String,
) -> Result<crate::history::reader::Workspace, String> {
    let result = probe(&connection, Some(&path)).await?;
    register_workspace(&state, &connection, result)
}

fn workspace_transport(
    connection: &SshConnectionInput,
    result: &SshConnectionProbe,
) -> Result<WslTransport, String> {
    if !result.dependencies.bash.available || !result.dependencies.python.available {
        return Err(
            "Remote Bash and Python3 are required; install missing dependencies first".into(),
        );
    }
    if !result.claude_available {
        return Err("Claude Code was not found in the remote login shell".into());
    }
    if result.claude_auth.status != "authenticated" {
        return Err("Claude Code login could not be confirmed; run claude auth login on the VPS and check again".into());
    }
    let mut tp = transport(&connection, &result.canonical_path)?;
    tp.engine_paths = result
        .claude_path
        .as_ref()
        .map(|path| HashMap::from([("claude".into(), path.clone())]))
        .unwrap_or_default();
    Ok(tp)
}

fn check_workspace_collision(
    db: &crate::db::Db,
    path: &str,
    tp: &WslTransport,
) -> Result<(), String> {
    check_workspace_collision_locked(&db.0.lock(), path, Some(tp))
}

fn paths_overlap(left: &str, right: &str) -> bool {
    let left = left.trim_end_matches('/');
    let right = right.trim_end_matches('/');
    left == right
        || left
            .strip_prefix(right)
            .is_some_and(|rest| rest.starts_with('/'))
        || right
            .strip_prefix(left)
            .is_some_and(|rest| rest.starts_with('/'))
}

fn check_workspace_collision_locked(
    conn: &rusqlite::Connection,
    path: &str,
    incoming: Option<&WslTransport>,
) -> Result<(), String> {
    let mut statement = conn
        .prepare("SELECT path, meta FROM workspaces")
        .map_err(|e| e.to_string())?;
    let rows = statement
        .query_map([], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, Option<String>>(1)?))
        })
        .map_err(|e| e.to_string())?;
    for row in rows {
        let (registered_path, raw) = row.map_err(|e| e.to_string())?;
        if !paths_overlap(path, &registered_path) {
            continue;
        }
        let metadata: Option<serde_json::Value> = raw
            .as_deref()
            .map(serde_json::from_str)
            .transpose()
            .map_err(|e| format!("Invalid existing workspace metadata: {e}"))?;
        let old_is_ssh = metadata
            .as_ref()
            .is_some_and(|meta| meta.get("ssh").is_some());
        if incoming.is_none() && !old_is_ssh {
            continue;
        }
        let old = metadata
            .as_ref()
            .map(wsl_transport::from_workspace_meta_checked)
            .transpose()?
            .flatten();
        let compatible = incoming.zip(old.as_ref()).is_some_and(|(new, old)| {
            old.distro.is_empty()
                && new.distro.is_empty()
                && old.host == new.host
                && old.user == new.user
                && old.port == new.port
        });
        if !compatible {
            return Err(format!(
                "Workspace path overlaps a local directory or another SSH host: {registered_path}"
            ));
        }
    }
    Ok(())
}

/// Called inside the workspace upsert lock so simultaneous registrations
/// cannot change which host owns an ancestor/descendant file path.
pub(crate) fn validate_workspace_registration(
    conn: &rusqlite::Connection,
    path: &str,
    metadata: Option<&serde_json::Value>,
) -> Result<(), String> {
    let incoming = if metadata.is_some_and(|meta| meta.get("ssh").is_some()) {
        metadata
            .map(wsl_transport::from_workspace_meta_checked)
            .transpose()?
            .flatten()
    } else {
        None
    };
    check_workspace_collision_locked(conn, path, incoming.as_ref())
}

fn register_workspace(
    state: &crate::AppState,
    connection: &SshConnectionInput,
    result: SshConnectionProbe,
) -> Result<crate::history::reader::Workspace, String> {
    let tp = workspace_transport(connection, &result)?;
    check_workspace_collision(&state.db, &result.canonical_path, &tp)?;
    let meta = serde_json::json!({"ssh": {
        "host":tp.host,"user":tp.user,"port":tp.port,"identityFile":tp.identity_file,
        "workspace":result.canonical_path,"enginePaths":tp.engine_paths
    }});
    crate::history::reader::add_workspace_inner(
        &state,
        &result.canonical_path,
        Some(meta),
        None,
        None,
    )
}

fn project_name(name: &str) -> Result<&str, String> {
    let name = name.trim();
    if name.is_empty()
        || matches!(name, "." | "..")
        || name.contains(['/', '\\', '\0', '\n', '\r'])
        || name.chars().any(char::is_control)
    {
        return Err("Project name must be a single directory name".into());
    }
    Ok(name)
}

fn create_directory_script(parent: &str, name: &str) -> String {
    format!(
        "python3 - {} {} <<'CCGUI_CREATE_PROJECT_PY'\n{}\nCCGUI_CREATE_PROJECT_PY\n",
        wsl_transport::sh_quote(parent),
        wsl_transport::sh_quote(name),
        CREATE_PROJECT_PY
    )
}

/// Create a new directory on the connected VPS, then open it as a workspace.
/// mkdir is exclusive: an existing project is never overwritten or adopted.
#[tauri::command]
pub async fn create_ssh_workspace(
    state: tauri::State<'_, crate::AppState>,
    connection: SshConnectionInput,
    parent_path: String,
    name: String,
) -> Result<crate::history::reader::Workspace, String> {
    let name = project_name(&name)?;
    let mut result = probe(&connection, Some(&parent_path)).await?;
    let tp = workspace_transport(&connection, &result)?;
    let path = format!("{}/{}", result.canonical_path.trim_end_matches('/'), name);
    check_workspace_collision(&state.db, &path, &tp)?;
    let output = wsl_transport::run_script_output(
        &tp,
        &create_directory_script(&result.canonical_path, name),
    )
    .await.map_err(|error| format!("Creating remote project {path} did not finish: {error}. Check whether the directory was created before retrying."))?;
    #[derive(Deserialize)]
    struct CreatedDirectory {
        path: Option<String>,
        error: Option<String>,
    }
    let created: CreatedDirectory = serde_json::from_str(&output)
        .map_err(|e| format!("Invalid remote project creation response: {e}"))?;
    if let Some(error) = created.error {
        return Err(error);
    }
    result.canonical_path = created
        .path
        .ok_or("Remote project creation returned no directory")?;
    result.directories.clear();
    let created_path = result.canonical_path.clone();
    register_workspace(&state, &connection, result).map_err(|error| {
        format!("Remote directory was created at {created_path}, but adding the workspace failed: {error}")
    })
}

/// Remote history is discovered alongside plugin-provided sources and uses
/// the existing remote page loader for streaming-format transcript parsing.
#[tauri::command]
pub async fn list_ssh_sessions(
    state: tauri::State<'_, crate::AppState>,
    workspace_path: String,
) -> Result<Vec<crate::history::SessionMeta>, String> {
    let tp = wsl_transport::transport_for_path(&state.db, &workspace_path)?
        .ok_or("Unknown SSH workspace")?;
    if !tp.distro.is_empty() {
        return Err("This command requires a Linux SSH workspace".into());
    }
    let root = tp
        .workspace
        .as_deref()
        .ok_or("Missing remote workspace root")?;
    let active = state.processes.active_run_count();
    let cache_key = format!(
        "{}:{}@{}:{}:{}",
        crate::paths::app_home().display(),
        tp.user,
        tp.host,
        tp.port,
        root
    );
    let cached = {
        let cache = HISTORY_CACHE
            .lock()
            .map_err(|_| "Remote history cache is unavailable")?;
        cache
            .get(&cache_key)
            .filter(|(at, count, _)| at.elapsed() < Duration::from_secs(2) && *count == active)
            .map(|(_, _, sessions)| sessions.clone())
    };
    if let Some(sessions) = cached {
        return Ok(sessions);
    }
    let script = format!(
        "python3 - {} <<'CCGUI_PY'\n{}\nCCGUI_PY\n",
        wsl_transport::sh_quote(root),
        SESSIONS_PY
    );
    let output = wsl_transport::run_readonly_script_output(&tp, &script).await?;
    let mut sessions: Vec<crate::history::SessionMeta> = serde_json::from_str(&output)
        .map_err(|e| format!("Invalid remote history response: {e}"))?;
    let conn = state.db.0.lock();
    sessions.retain(|s| {
        conn.query_row(
            "SELECT 1 FROM session_archives WHERE engine=?1 AND session_id=?2",
            rusqlite::params![s.engine, s.session_id],
            |_| Ok(()),
        )
        .is_err()
    });
    for session in &mut sessions {
        if let Ok(model) = conn.query_row(
            "SELECT model FROM session_models WHERE engine=?1 AND session_id=?2",
            rusqlite::params![session.engine, session.session_id],
            |r| r.get(0),
        ) {
            session.model = model;
        }
        if let Ok(effort) = conn.query_row(
            "SELECT effort FROM session_efforts WHERE engine=?1 AND session_id=?2",
            rusqlite::params![session.engine, session.session_id],
            |r| r.get(0),
        ) {
            session.effort = effort;
        }
    }
    drop(conn);
    let mut cache = HISTORY_CACHE
        .lock()
        .map_err(|_| "Remote history cache is unavailable")?;
    if cache.len() >= 64 {
        cache.retain(|_, (at, _, _)| at.elapsed() < Duration::from_secs(2));
    }
    cache.insert(cache_key, (Instant::now(), active, sessions.clone()));
    Ok(sessions)
}

fn probe_script(path: &str) -> String {
    // Missing Python must be a visible dependency result, not an unparseable
    // shell error. Values in this fallback are fixed booleans, never paths.
    let missing = r#"if ! command -v python3 >/dev/null 2>&1; then
b=false; g=false; c=false
command -v bash >/dev/null 2>&1 && b=true
command -v git >/dev/null 2>&1 && g=true
command -v claude >/dev/null 2>&1 && c=true
printf '{"home":"","claudeAvailable":%s,"claudeVersion":null,"canonicalPath":"","directories":[],"dependencies":{"bash":{"available":%s,"version":null},"python":{"available":false,"version":null},"git":{"available":%s,"version":null},"claude":{"available":%s,"version":null}},"claudeAuth":{"status":"unknown","method":null}}\n' "$c" "$b" "$g" "$c"
exit 0
fi
"#;
    format!(
        "{missing}python3 - {} <<'CCGUI_PY'\n{}\nCCGUI_PY\n",
        wsl_transport::sh_quote(path),
        PROBE_PY
    )
}

const PROBE_PY: &str = r#"
import json, os, shutil, subprocess, sys
from concurrent.futures import ThreadPoolExecutor
home = os.path.expanduser('~')
path = os.path.realpath(os.path.expanduser(sys.argv[1]))
if not os.path.isdir(path): raise RuntimeError('Remote directory does not exist: ' + path)
programs = {'bash':shutil.which('bash'), 'python':sys.executable, 'git':shutil.which('git'), 'claude':shutil.which('claude')}
def version(name):
    program = programs[name]
    value = None
    if program:
        try:
            result = subprocess.run([program, '--version'], stdin=subprocess.DEVNULL, stdout=subprocess.PIPE, stderr=subprocess.DEVNULL, text=True, timeout=4)
            if result.returncode == 0: value = result.stdout.splitlines()[0][:256] if result.stdout.splitlines() else None
        except (OSError, subprocess.TimeoutExpired): pass
    return name, {'available':bool(program), 'version':value}
with ThreadPoolExecutor(max_workers=4) as executor: dependencies = dict(executor.map(version, programs))
auth = {'status':'unknown','method':None}
claude = programs['claude']
if claude:
    try:
        # Official CLI: auth status emits JSON by default, 0 logged in / 1 out.
        # Never expose its account/config fields or raw diagnostics.
        result = subprocess.run([claude, 'auth', 'status'], stdin=subprocess.DEVNULL, stdout=subprocess.PIPE, stderr=subprocess.DEVNULL, text=True, timeout=4)
        if len(result.stdout.encode('utf-8')) <= 65536:
            value = json.loads(result.stdout)
            if isinstance(value, dict):
                logged_in = value.get('loggedIn')
                if logged_in is True and result.returncode == 0: auth['status'] = 'authenticated'
                elif logged_in is False and result.returncode == 1: auth['status'] = 'unauthenticated'
                method = value.get('authMethod')
                if method in ('none','claude.ai','oauth_token','api_key','api_key_helper','third_party'): auth['method'] = method
    except (OSError, ValueError, subprocess.TimeoutExpired): pass
directories = []
with os.scandir(path) as entries:
    for entry in entries:
        if not entry.name.startswith('.') and entry.is_dir(): directories.append({'name':entry.name,'path':entry.path})
directories.sort(key=lambda item:item['name'].casefold())
print(json.dumps({'home':home,'claudeAvailable':bool(claude),'claudeVersion':dependencies['claude']['version'],'claudePath':claude,'canonicalPath':path,'directories':directories,'dependencies':dependencies,'claudeAuth':auth}, ensure_ascii=False))
"#;

const CREATE_PROJECT_PY: &str = r#"
import json, os, sys
try:
    parent, name = os.path.realpath(sys.argv[1]), sys.argv[2]
    if not os.path.isdir(parent): raise ValueError('Remote parent directory does not exist')
    if not name or name in ('.', '..') or '/' in name or '\\' in name or any(ord(c) < 32 or ord(c) == 127 for c in name):
        raise ValueError('Project name must be a single directory name')
    path = os.path.join(parent, name)
    os.mkdir(path)
    response = {'path': path}
except FileExistsError:
    response = {'error': 'A file or directory with this project name already exists'}
except Exception as error:
    response = {'error': str(error)}
print(json.dumps(response, ensure_ascii=False))
"#;

const SESSIONS_PY: &str = r#"
import datetime, json, os, re, sys
workspace = sys.argv[1]
encoded = re.sub(r'[^a-zA-Z0-9-]', '-', workspace)
root = os.path.join(os.environ.get('CLAUDE_CONFIG_DIR',os.path.expanduser('~/.claude')), 'projects', encoded)
sessions = []
def text(content):
    if isinstance(content,str): return content
    if isinstance(content,list): return '\n'.join(p.get('text','') for p in content if isinstance(p,dict) and p.get('type')=='text')
    return ''
def timestamp(value):
    if not value: return None
    try: return int(datetime.datetime.fromisoformat(value.replace('Z','+00:00')).timestamp()*1000)
    except (ValueError,TypeError): return None
if os.path.isdir(root):
    for entry in os.scandir(root):
        if not entry.is_file() or not entry.name.endswith('.jsonl') or entry.name.startswith('agent-'): continue
        stat = entry.stat()
        if stat.st_size > 64*1024*1024: continue
        title, preview, count, created, updated, model = '', '', 0, None, None, None
        with open(entry.path, encoding='utf-8',errors='replace') as handle:
            for line in handle:
                try: row=json.loads(line)
                except (ValueError,TypeError): continue
                if not isinstance(row,dict): continue
                message=row.get('message') or {}
                if not isinstance(message,dict): continue
                content=text(message.get('content')).strip()
                role=row.get('type')
                if role not in ('user','assistant') or row.get('isMeta') or not content or content.startswith('<local-command-'): continue
                if role=='user' and not title: title=content[:160]
                preview=content[:240]
                count+=1
                ts=timestamp(row.get('timestamp'))
                if ts: created=created or ts; updated=ts
                model=message.get('model') or model
        if not count: continue
        sessions.append({'engine':'claude','sessionId':entry.name[:-6],'workspacePath':workspace,'filePath':entry.path,'fileSize':stat.st_size,'fileMtimeMs':int(stat.st_mtime*1000),'title':title or 'Claude Code','preview':preview,'createdAt':created,'updatedAt':updated or int(stat.st_mtime*1000),'messageCount':count,'pinned':False,'customTitle':None,'model':None,'effort':None,'provider':None,'remote':True,'remotePath':entry.path})
sessions.sort(key=lambda session:session['updatedAt'] or 0, reverse=True)
print(json.dumps(sessions,ensure_ascii=False))
"#;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missing_python_is_a_structured_visible_probe_result() {
        let output = std::process::Command::new("/bin/sh")
            .args(["-c", &probe_script("~")])
            .env("PATH", "/ccgui-no-programs")
            .output()
            .unwrap();
        assert!(output.status.success());
        let probe: SshConnectionProbe = serde_json::from_slice(&output.stdout).unwrap();
        assert!(!probe.dependencies.python.available);
        assert!(!probe.dependencies.bash.available);
        assert_eq!(probe.claude_auth.status, "unknown");
        assert!(probe.directories.is_empty());
    }

    #[test]
    fn auth_probe_normalizes_only_confirmed_status_and_never_account_fields() {
        use std::io::Write;
        let scratch =
            std::env::temp_dir().join(format!("ccgui-auth-probe-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir(&scratch).unwrap();
        let runner = r#"import json, subprocess, sys, types
from unittest.mock import patch
source = sys.stdin.read()
logged, code = json.loads(sys.argv[2])
def run(args, **kwargs):
    assert '-p' not in args and '--print' not in args
    if args[1:] == ['auth','status']:
        return types.SimpleNamespace(returncode=code, stdout=json.dumps({'loggedIn':logged,'authMethod':'claude.ai','email':'PRIVATE_EMAIL','accessToken':'PRIVATE_TOKEN'}))
    return types.SimpleNamespace(returncode=0, stdout='test-version\n')
with patch('shutil.which', side_effect=lambda name: '/fake/'+name if name != 'git' else None), patch('subprocess.run', side_effect=run):
    exec(compile(source,'probe','exec'))
"#;
        for (logged, code, expected) in [
            (true, 0, "authenticated"),
            (false, 1, "unauthenticated"),
            (false, 0, "unknown"),
            (true, 1, "unknown"),
        ] {
            let mut child = std::process::Command::new("python3")
                .args([
                    "-c",
                    runner,
                    scratch.to_str().unwrap(),
                    &serde_json::json!([logged, code]).to_string(),
                ])
                .stdin(std::process::Stdio::piped())
                .stdout(std::process::Stdio::piped())
                .spawn()
                .unwrap();
            child
                .stdin
                .take()
                .unwrap()
                .write_all(PROBE_PY.as_bytes())
                .unwrap();
            let output = child.wait_with_output().unwrap();
            assert!(output.status.success());
            let text = String::from_utf8(output.stdout).unwrap();
            assert!(!text.contains("PRIVATE_EMAIL"));
            assert!(!text.contains("PRIVATE_TOKEN"));
            let probe: SshConnectionProbe = serde_json::from_str(&text).unwrap();
            assert_eq!(probe.claude_auth.status, expected);
            assert!(!probe.dependencies.git.available);
        }
        std::fs::remove_dir(scratch).unwrap();
    }

    #[test]
    fn registration_requires_runtime_and_login_but_git_is_optional() {
        let mut result: SshConnectionProbe = serde_json::from_value(serde_json::json!({"home":"/home/dev","canonicalPath":"/home/dev/project","directories":[],"claudeAvailable":true,"claudeVersion":"test","claudePath":"/bin/claude","dependencies":{"bash":{"available":true,"version":null},"python":{"available":true,"version":null},"git":{"available":false,"version":null},"claude":{"available":true,"version":null}},"claudeAuth":{"status":"authenticated","method":"claude.ai"}})).unwrap();
        let connection = SshConnectionInput {
            host: "example.invalid".into(),
            user: "dev".into(),
            port: 22,
            identity_file: None,
        };
        assert!(workspace_transport(&connection, &result).is_ok());
        result.claude_auth.status = "unknown".into();
        assert!(workspace_transport(&connection, &result).is_err());
        result.claude_auth.status = "authenticated".into();
        result.dependencies.python.available = false;
        assert!(workspace_transport(&connection, &result).is_err());
    }
    #[test]
    fn connection_rejects_option_injection_and_zero_port() {
        let mut connection = SshConnectionInput {
            host: "127.0.0.1".into(),
            user: "dev".into(),
            port: 22,
            identity_file: None,
        };
        assert!(transport(&connection, "/home/dev/project").is_ok());
        connection.host = "-oProxyCommand=evil".into();
        assert!(transport(&connection, "/home/dev/project").is_err());
        connection.host = "127.0.0.1".into();
        connection.port = 0;
        assert!(transport(&connection, "/home/dev/project").is_err());
    }

    #[test]
    fn new_project_names_cannot_escape_parent_directory() {
        for name in [
            "",
            " ",
            ".",
            "..",
            "../escape",
            "/tmp/project",
            "nested/name",
            "nested\\name",
            "line\nbreak",
            "nul\0byte",
        ] {
            assert!(project_name(name).is_err(), "accepted {name:?}");
        }
        assert_eq!(project_name("  我的项目  ").unwrap(), "我的项目");
        assert!(project_name("project ' $(touch INJECTION)").is_ok());
    }

    #[test]
    fn creating_remote_project_is_exclusive_and_quotes_names() {
        let scratch =
            std::env::temp_dir().join(format!("ccgui-create-project-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&scratch).unwrap();
        let scratch = scratch.canonicalize().unwrap();
        let name = "中文 ' $(touch INJECTION) `id`";
        let run = |name: &str| {
            let output = std::process::Command::new("bash")
                .args([
                    "-c",
                    &create_directory_script(scratch.to_str().unwrap(), name),
                ])
                .output()
                .unwrap();
            assert!(output.status.success());
            serde_json::from_slice::<serde_json::Value>(&output.stdout).unwrap()
        };
        let result = run(name);
        assert_eq!(result["path"], scratch.join(name).to_str().unwrap());
        assert!(scratch.join(name).is_dir());
        std::fs::write(scratch.join(name).join("keep.txt"), "preserve").unwrap();
        assert!(run(name)["error"]
            .as_str()
            .unwrap()
            .contains("already exists"));
        assert_eq!(
            std::fs::read_to_string(scratch.join(name).join("keep.txt")).unwrap(),
            "preserve"
        );
        assert!(run("../escape")["error"].is_string());
        assert!(!scratch.join("INJECTION").exists());
        std::fs::remove_dir_all(scratch).unwrap();
    }

    fn ssh_metadata(path: &str, host: &str, user: &str, port: u16) -> serde_json::Value {
        serde_json::json!({"ssh": {
            "host": host, "user": user, "port": port, "workspace": path
        }})
    }

    fn registration_fixture(
        path: &str,
        metadata: Option<serde_json::Value>,
    ) -> rusqlite::Connection {
        let conn = rusqlite::Connection::open_in_memory().unwrap();
        conn.execute_batch("CREATE TABLE workspaces(path TEXT PRIMARY KEY, meta TEXT)")
            .unwrap();
        conn.execute(
            "INSERT INTO workspaces(path, meta) VALUES(?1, ?2)",
            rusqlite::params![path, metadata.map(|value| value.to_string())],
        )
        .unwrap();
        conn
    }

    #[test]
    fn ssh_registration_rejects_local_ancestor_descendant_and_exact_collisions() {
        let conn = registration_fixture("/srv/projects", None);
        for incoming in ["/srv", "/srv/projects", "/srv/projects/new"] {
            let metadata = ssh_metadata(incoming, "host-a", "dev", 22);
            let error =
                validate_workspace_registration(&conn, incoming, Some(&metadata)).unwrap_err();
            assert!(error.contains("overlaps"), "path={incoming}, error={error}");
        }
        // Existing local-local nesting keeps its original behavior, and a
        // sibling sharing a string prefix is not an ancestor collision.
        assert!(validate_workspace_registration(&conn, "/srv/projects/new", None).is_ok());
        let sibling = ssh_metadata("/srv/project", "host-a", "dev", 22);
        assert!(validate_workspace_registration(&conn, "/srv/project", Some(&sibling)).is_ok());
    }

    #[test]
    fn local_registration_cannot_take_over_an_ssh_root_or_its_ancestors() {
        let conn = registration_fixture(
            "/srv/projects",
            Some(ssh_metadata("/srv/projects", "host-a", "dev", 22)),
        );
        for incoming in ["/srv", "/srv/projects", "/srv/projects/new"] {
            assert!(
                validate_workspace_registration(&conn, incoming, None).is_err(),
                "accepted {incoming}"
            );
        }
        assert!(validate_workspace_registration(&conn, "/srv/project", None).is_ok());
    }

    #[test]
    fn cross_host_overlapping_roots_are_rejected_in_both_directions() {
        let conn = registration_fixture(
            "/srv/projects",
            Some(ssh_metadata("/srv/projects", "host-a", "dev", 22)),
        );
        for incoming in ["/srv", "/srv/projects", "/srv/projects/new"] {
            let metadata = ssh_metadata(incoming, "host-b", "dev", 22);
            assert!(
                validate_workspace_registration(&conn, incoming, Some(&metadata)).is_err(),
                "accepted {incoming}"
            );
        }
        let sibling = ssh_metadata("/srv/projects-other", "host-b", "dev", 22);
        assert!(
            validate_workspace_registration(&conn, "/srv/projects-other", Some(&sibling)).is_ok()
        );
    }

    #[test]
    fn nested_and_existing_roots_on_the_same_ssh_connection_are_compatible() {
        let conn = registration_fixture(
            "/srv/projects",
            Some(ssh_metadata("/srv/projects", "host-a", "dev", 2222)),
        );
        for incoming in ["/srv", "/srv/projects", "/srv/projects/new"] {
            let metadata = ssh_metadata(incoming, "host-a", "dev", 2222);
            assert!(
                validate_workspace_registration(&conn, incoming, Some(&metadata)).is_ok(),
                "rejected {incoming}"
            );
        }
    }

    #[test]
    fn exact_ssh_root_cannot_change_user_or_port() {
        let root = "/srv/projects";
        let conn = registration_fixture(root, Some(ssh_metadata(root, "host-a", "dev", 22)));
        for (user, port) in [("other", 22), ("dev", 2222)] {
            let metadata = ssh_metadata(root, "host-a", user, port);
            assert!(
                validate_workspace_registration(&conn, root, Some(&metadata)).is_err(),
                "accepted user={user} port={port}"
            );
        }
    }

    #[test]
    fn malformed_ssh_metadata_does_not_fall_back_to_local_registration() {
        let conn = registration_fixture("/srv/projects", Some(serde_json::json!({"ssh": null})));
        assert!(validate_workspace_registration(&conn, "/srv/projects/new", None).is_err());
        let conn = registration_fixture("/unrelated", None);
        let invalid = serde_json::json!({"ssh": {"host": "-oProxyCommand=evil"}});
        assert!(validate_workspace_registration(&conn, "/srv/projects", Some(&invalid)).is_err());
    }

    #[test]
    fn creating_remote_project_preserves_files_symlinks_and_missing_parents() {
        let scratch =
            std::env::temp_dir().join(format!("ccgui-create-preserve-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&scratch).unwrap();
        let scratch = scratch.canonicalize().unwrap();
        let run = |parent: &std::path::Path, name: &str| {
            let output = std::process::Command::new("bash")
                .args([
                    "-c",
                    &create_directory_script(parent.to_str().unwrap(), name),
                ])
                .output()
                .unwrap();
            assert!(output.status.success());
            serde_json::from_slice::<serde_json::Value>(&output.stdout).unwrap()
        };
        let existing = scratch.join("existing.txt");
        std::fs::write(&existing, "preserve").unwrap();
        assert!(run(&scratch, "existing.txt")["error"]
            .as_str()
            .unwrap()
            .contains("already exists"));
        assert_eq!(std::fs::read_to_string(&existing).unwrap(), "preserve");
        #[cfg(unix)]
        {
            let symlink = scratch.join("existing-link");
            std::os::unix::fs::symlink(&existing, &symlink).unwrap();
            assert!(run(&scratch, "existing-link")["error"]
                .as_str()
                .unwrap()
                .contains("already exists"));
            assert!(std::fs::symlink_metadata(&symlink)
                .unwrap()
                .file_type()
                .is_symlink());
            assert_eq!(std::fs::read_to_string(&existing).unwrap(), "preserve");
        }
        let missing = scratch.join("missing-parent");
        assert!(run(&missing, "new")["error"].is_string());
        assert!(!missing.exists());
        std::fs::remove_dir_all(scratch).unwrap();
    }
}
