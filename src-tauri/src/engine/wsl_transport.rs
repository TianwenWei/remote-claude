//! SSH engine transport for native Linux (`meta.ssh`) and the existing
//! Windows/WSL plugin (`meta.wsl`). Both upload a private command script and
//! keep prompt/control stdin and NDJSON stdout attached to the native run.
//!
//! Native Linux uses the remote login shell, strict known-host validation,
//! and an optional explicit identity file. A remote supervisor owns a fresh
//! CLI process group and watches its sshd parent, terminating the CLI and
//! tool children after disconnect. WSL retains its existing wsl.exe path.
//! Prompts travel over stdin and are never embedded in uploaded scripts.

use std::collections::HashMap;
use std::path::PathBuf;
use std::process::Stdio;
use std::time::Duration;

use tokio::process::Command;

/// 远程短调用(脚本上传/模型目录/历史回放拉取)的全局 deadline。
/// ssh_options 的 ConnectTimeout 只覆盖 TCP 建连;远端建连后挂起(wsl.exe
/// 无响应/管道写满)没有它会让 tauri 命令 future 永不 resolve,前端无
/// 取消路径,发送/模型选择/历史翻页全部卡死到 app 重启。引擎会话本体
/// (wrap 出的长驻 run 命令)不在此列——它的存活期就是 turn 的存活期。
const REMOTE_CALL_TIMEOUT: Duration = Duration::from_secs(30);

/// `meta.wsl` 的形状(插件 ccgui-plugin-wsl 写入;其他插件可同构复用)。
#[derive(Debug, Clone)]
pub struct WslTransport {
    /// Windows 宿主地址(IP/主机名)。
    pub host: String,
    pub port: u16,
    pub user: String,
    /// 发行版名(wsl.exe -d)。
    pub distro: String,
    /// SSH ControlMaster 套接字路径;None = 纯 BatchMode(key 认证)。
    pub control_path: Option<String>,
    /// Native Linux SSH key. Empty distro selects Linux rather than WSL.
    pub identity_file: Option<String>,
    /// 引擎 bin 在发行版内的绝对路径(bin 名 → 路径,插件探针写入)。
    pub engine_paths: HashMap<String, String>,
    /// 工作区在发行版内的路径(`cd` 目标;缺省 = 发行版默认 cwd)。
    pub workspace: Option<String>,
}

/// Compatibility parser. Actions use the checked parser/path lookup below
/// so invalid remote metadata cannot silently select local execution.
pub fn from_workspace_meta(meta: &serde_json::Value) -> Option<WslTransport> {
    from_workspace_meta_checked(meta).ok().flatten()
}

/// A registered remote workspace must never silently turn into a local one.
pub fn from_workspace_meta_checked(
    meta: &serde_json::Value,
) -> Result<Option<WslTransport>, String> {
    if !meta.is_object() || (meta.get("ssh").is_some() && meta.get("wsl").is_some()) {
        return Err("Invalid or ambiguous remote workspace metadata".into());
    }
    let Some(remote) = meta.get("ssh").or_else(|| meta.get("wsl")) else {
        return Ok(None);
    };
    parse_remote_meta(remote, meta.get("ssh").is_some())
        .map(Some)
        .ok_or_else(|| "Invalid remote workspace metadata; reconnect this workspace".to_string())
}

fn parse_remote_meta(wsl: &serde_json::Value, linux: bool) -> Option<WslTransport> {
    let host = wsl.get("host")?.as_str()?.trim().to_string();
    if !is_safe_host(&host) {
        return None;
    }
    let user = wsl.get("user")?.as_str()?.trim().to_string();
    if !is_safe_user(&user) {
        return None;
    }
    let distro = if linux {
        String::new()
    } else {
        wsl.get("distro")?.as_str()?.trim().to_string()
    };
    if !linux && !is_safe_distro(&distro) {
        return None;
    }
    let workspace = wsl
        .get("workspace")
        .and_then(|v| v.as_str())
        .map(str::to_string);
    if linux && !workspace.as_deref().is_some_and(is_safe_linux_path) {
        return None;
    }
    let identity_file = match wsl.get("identityFile") {
        None | Some(serde_json::Value::Null) => None,
        Some(value) => {
            let path = value.as_str()?;
            if !std::path::Path::new(path).is_absolute() || path.contains(['\0', '\n', '\r']) {
                return None;
            }
            Some(path.to_string())
        }
    };
    let port = match wsl.get("port") {
        Some(v) => v.as_u64()?.try_into().ok()?,
        None => 22,
    };
    if port == 0 {
        return None;
    }
    let engine_paths = wsl
        .get("enginePaths")
        .and_then(|v| v.as_object())
        .map(|o| {
            o.iter()
                .filter_map(|(k, v)| {
                    let path = v.as_str()?.trim();
                    (!path.is_empty()).then(|| (k.clone(), path.to_string()))
                })
                .collect()
        })
        .unwrap_or_default();
    Some(WslTransport {
        host,
        port,
        user,
        distro,
        control_path: if linux {
            None
        } else {
            wsl.get("controlPath")
                .and_then(|v| v.as_str())
                .map(str::to_string)
                .filter(|s| !s.trim().is_empty())
        },
        identity_file,
        engine_paths,
        workspace: wsl
            .get("workspace")
            .and_then(|v| v.as_str())
            .map(str::to_string)
            .filter(|s| !s.trim().is_empty()),
    })
}

pub fn is_safe_linux_path(path: &str) -> bool {
    path.starts_with('/')
        && !path.contains(['\0', '\n', '\r'])
        && !path.split('/').any(|part| part == "..")
}

/// Remote host/user fields are checked before they reach SSH argv or the
/// Windows-side WSL shell. Checked callers surface invalid registration.

/// ssh 登录名:Linux 用户名字符集,且不得以 `-` 开头 —— 否则 `user@host`
/// 整体会被 ssh 的 getopt 当成选项吞掉(`-oProxyCommand=…` → 本机命令执行)。
pub(super) fn is_safe_user(user: &str) -> bool {
    !user.is_empty()
        && !user.starts_with('-')
        && user
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-'))
}

/// 主机地址:IP/主机名/IPv6,同样不得以 `-` 开头(getopt 选项注入)。
pub(super) fn is_safe_host(host: &str) -> bool {
    !host.is_empty()
        && !host.starts_with('-')
        && host
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-' | ':'))
}

/// 发行版名:拼进远端 DefaultShell 解释的双引号串,PowerShell 下 `$(…)`/
/// 反引号、cmd 下 `%VAR%` 在引号内仍会求值 —— 只放行字母数字与 `._- `。
fn is_safe_distro(distro: &str) -> bool {
    !distro.is_empty()
        && distro
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-' | ' '))
}

/// 发行版内工作区路径白名单(脚本内不加引号直排 `cd <ws>`:bash 对行首
/// `~` 原生 tilde 展开;空格/`$`/`;` 等一律拒绝)。send 路径的
/// build_script 与 catalog 路径的 models/wsl.rs 共用同一规则。
pub(crate) fn is_safe_workspace(ws: &str) -> bool {
    !ws.is_empty()
        && ws
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '/' | '_' | '.' | '-' | '~'))
}

/// 单个 argv 元素的 POSIX 单引号包裹(bash 脚本内)。
pub(crate) fn sh_quote(s: &str) -> String {
    format!("'{}'", s.replace('\'', r"'\''"))
}

/// 从 tokio Command 抽取 program + args + 显式 env(构造点都在本 crate 内,
/// get_envs 只含 build_command 显式设置的项,不含继承环境)。env 经脚本头
/// `export` 跨机传递(MAX_THINKING_TOKENS 等;值一律 sh_quote)。
fn program_args_env(command: &Command) -> (String, Vec<String>, Vec<(String, String)>) {
    let std_cmd = command.as_std();
    let program = std_cmd.get_program().to_string_lossy().into_owned();
    let args = std_cmd
        .get_args()
        .map(|a| a.to_string_lossy().into_owned())
        .collect();
    let envs = std_cmd
        .get_envs()
        .filter_map(|(k, v)| {
            let key = k.to_string_lossy().into_owned();
            // env 键必须是合法 shell 标识符,否则宁可丢掉也不进脚本
            let valid = key.starts_with(|c: char| c.is_ascii_alphabetic() || c == '_')
                && key.chars().all(|c| c.is_ascii_alphanumeric() || c == '_');
            let value = v?.to_string_lossy().into_owned();
            valid.then(|| (key, value))
        })
        .collect();
    (program, args, envs)
}

/// ssh 公共选项(key 认证 BatchMode;ControlMaster 存在则复用免密)。
fn ssh_options(transport: &WslTransport) -> Vec<String> {
    let mut opts = vec![
        "-o".to_string(),
        // Keep the existing WSL plugin's first-connect policy. Native Linux
        // requires an already trusted known_hosts entry on every connection.
        if transport.distro.is_empty() {
            "StrictHostKeyChecking=yes"
        } else {
            "StrictHostKeyChecking=accept-new"
        }
        .to_string(),
        "-o".to_string(),
        "ConnectTimeout=10".to_string(),
        "-p".to_string(),
        transport.port.to_string(),
    ];
    if transport.distro.is_empty() {
        // File/Git helpers send sizeable scripts over stdin. Compression
        // reduces bulk traffic through local VPN/TUN network adapters.
        opts.extend(["-o".to_string(), "Compression=yes".to_string()]);
    }
    if let Some(identity) = &transport.identity_file {
        opts.extend([
            "-i".to_string(),
            identity.clone(),
            "-o".to_string(),
            "IdentitiesOnly=yes".to_string(),
        ]);
    }
    opts.extend([
        "-o".to_string(),
        "ServerAliveInterval=15".to_string(),
        "-o".to_string(),
        "ServerAliveCountMax=3".to_string(),
    ]);
    match transport
        .control_path
        .as_ref()
        .filter(|_| !transport.distro.is_empty())
    {
        // Master connection exists → no password prompt will occur; BatchMode
        // keeps a dead master from hanging the spawn waiting on input.
        Some(cp) => {
            opts.push("-o".to_string());
            opts.push(format!("ControlPath={cp}"));
            opts.push("-o".to_string());
            opts.push("BatchMode=yes".to_string());
        }
        None => {
            opts.push("-o".to_string());
            opts.push("BatchMode=yes".to_string());
        }
    }
    opts
}

fn ssh_target(transport: &WslTransport) -> String {
    format!("{}@{}", transport.user, transport.host)
}

/// 公共 ssh 命令骨架:选项 + `--` + destination。`--` 终结 getopt 解析,
/// destination 永远不被当成选项(user/host 另有白名单,这里是纵深防御)。
pub(crate) fn base_ssh_command(transport: &WslTransport) -> Command {
    ssh_command_with_control(transport, None)
}

/// Only explicitly read-only RPCs reuse a master. OpenSSH keeps the first
/// supplied option, so these options replace the dedicated mode at construction.
fn readonly_ssh_command(transport: &WslTransport) -> Result<Command, String> {
    let control = if transport.distro.is_empty() {
        super::ssh_rpc_mux::control_path(
            &transport.host,
            &transport.user,
            transport.port,
            transport.identity_file.as_deref(),
        )?
    } else {
        None
    };
    Ok(ssh_command_with_control(transport, control.as_deref()))
}

fn ssh_command_with_control(
    transport: &WslTransport,
    control: Option<&std::path::Path>,
) -> Command {
    let mut command = Command::new("ssh");
    for opt in ssh_options(transport) {
        command.arg(opt);
    }
    if transport.distro.is_empty() {
        if let Some(path) = control {
            command.args(["-o", "ControlMaster=auto", "-o", "ControlPersist=60", "-o"]);
            command.arg(format!("ControlPath={}", path.display()));
            command.args([
                "-o",
                "ForwardAgent=no",
                "-o",
                "ForwardX11=no",
                "-o",
                "ClearAllForwardings=yes",
                "-o",
                "RequestTTY=no",
            ]);
        } else {
            // Cancellation watches this session's sshd parent. A user's
            // ControlMaster would outlive a closed multiplex channel.
            command.args(["-o", "ControlMaster=no", "-o", "ControlPath=none"]);
        }
    }
    command.arg("--");
    command.arg(ssh_target(transport));
    command
}

pub(crate) fn remote_command_string(transport: &WslTransport, remote_argv: &[&str]) -> String {
    if transport.distro.is_empty() {
        let inner = remote_argv
            .iter()
            .map(|arg| sh_quote(arg))
            .collect::<Vec<_>>()
            .join(" ");
        return format!("bash -lc {}", sh_quote(&format!("exec {inner}")));
    }
    let distro = transport.distro.replace('"', "");
    let joined = remote_argv
        .iter()
        .map(|a| {
            // Windows-side quoting for the string the remote DefaultShell will
            // hand to wsl.exe: double quotes, no $ / backtick / | inside (the
            // caller guarantees the safe charset: tee/bash 与 UUID 路径是固定词,
            // distro 在 from_workspace_meta 已过白名单,replace 只是纵深防御).
            format!("\"{a}\"")
        })
        .collect::<Vec<_>>()
        .join(" ");
    format!("wsl.exe -d \"{distro}\" -- {joined}")
}

#[cfg(test)]
fn wsl_command_string(transport: &WslTransport, remote_argv: &[&str]) -> String {
    remote_command_string(transport, remote_argv)
}

/// 把本机 argv[0] 解析成发行版内的可执行路径:
/// 1. `enginePaths` 表命中(program 的 basename 或全等 program);
/// 2. 否则交给脚本内 `command -v` 兜底解析(远端 PATH 语义)。
fn resolve_remote_program(program: &str, transport: &WslTransport) -> String {
    let base = PathBuf::from(program)
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| program.to_string());
    if let Some(p) = transport
        .engine_paths
        .get(program)
        .or_else(|| transport.engine_paths.get(&base))
    {
        return p.clone();
    }
    format!("__RESOLVE__{base}")
}

/// 生成发行版内执行的脚本文本。workspace 路径来自插件登记(白名单见
/// is_safe_workspace),**不加引号**直排 —— bash 对行首 `~` 原生 tilde
/// 展开(tmd 2026-09-13 真机验证形态;引号内 `~` 不展开,手动 `$HOME`
/// 拼接是 Fragile 的)。
fn build_script(
    program: &str,
    args: &[String],
    envs: &[(String, String)],
    transport: &WslTransport,
) -> String {
    let mut script = String::new();
    script.push_str("set -e\n");
    // 脚本落盘即删:bash 对已打开的 fd 继续读取不受 unlink 影响(exec 替换
    // 进程镜像也能删,EXIT trap 在 exec 下不会触发,故不用 trap)。
    script.push_str("rm -f \"$0\"\n");
    // build_command 显式设置的 env 跨机传递(键已限 shell 标识符,值一律
    // 单引号包裹):MAX_THINKING_TOKENS / GROK_DISABLE_AUTOUPDATER 等。
    for (key, value) in envs {
        script.push_str(&format!("export {key}={}\n", sh_quote(value)));
    }
    if let Some(ws) = &transport.workspace {
        if transport.distro.is_empty() {
            if !is_safe_linux_path(ws) {
                return "echo 'Invalid remote workspace' >&2; exit 60\n".into();
            }
            script.push_str(&format!("cd -- {} || exit 61\n", sh_quote(ws)));
        } else if !is_safe_workspace(ws) {
            return format!("echo 'workspace 路径含不支持的字符' >&2; exit 60\n");
        } else {
            script.push_str(&format!("cd {ws} || exit 61\n"));
        }
    }
    let resolved = resolve_remote_program(program, transport);
    let invocation_start;
    if let Some(base) = resolved.strip_prefix("__RESOLVE__") {
        // 登录 shell 解析(~/.profile 后的 PATH):非登录 `command -v`
        // 探不到 ~/.local/bin(tmd 2026-09-12 实测),引擎常装在那。
        script.push_str(&format!(
            "bin=$(bash -lc {}) || exit 62\n",
            sh_quote(&format!("command -v {}", sh_quote(base)))
        ));
        if transport.distro.is_empty() {
            script.push_str("export bin\n");
        }
        invocation_start = script.len();
        script.push_str("exec \"$bin\"");
    } else {
        invocation_start = script.len();
        script.push_str("exec ");
        script.push_str(&sh_quote(&resolved));
    }
    for a in args {
        script.push(' ');
        script.push_str(&sh_quote(a));
    }
    script.push('\n');
    if transport.distro.is_empty() {
        // A non-PTY SSH disconnect need not HUP the CLI. Keep a supervisor
        // attached to sshd and terminate the isolated CLI process group if
        // that parent disappears. Explicit stdin redirection preserves the
        // stream-json permission/answer channel for the background child.
        let invocation = script.split_off(invocation_start);
        script.push_str(&format!(
            "parent=$PPID\nsetsid bash -c {} <&0 &\nworker=$!\n(while kill -0 \"$parent\" 2>/dev/null; do sleep 1; done; kill -TERM -- -\"$worker\" 2>/dev/null; sleep 2; kill -KILL -- -\"$worker\" 2>/dev/null) < /dev/null > /dev/null 2>&1 &\nwatcher=$!\ncleanup() {{ kill \"$watcher\" 2>/dev/null || true; kill -TERM -- -\"$worker\" 2>/dev/null || true; for n in 1 2 3 4 5 6 7 8 9 10; do kill -0 -- -\"$worker\" 2>/dev/null || return 0; sleep 0.2; done; kill -KILL -- -\"$worker\" 2>/dev/null || true; }}\ntrap cleanup EXIT HUP TERM INT\nset +e\nwait \"$worker\"\nstatus=$?\nexit \"$status\"\n",
            sh_quote(&invocation)
        ));
    }
    script
}

/// 把脚本经 ssh stdin 直写发行版 `tee`(脚本内容全程不进命令串,零转义
/// 需求;命令串只有固定词)。任何失败返回 Err(spawn 前失败,turn 直接报错)。
async fn upload_script(
    transport: &WslTransport,
    script_body: &str,
    remote_path: &str,
) -> Result<(), String> {
    // Uploads are short SSH connections too. Release the shared budget before
    // spawning the dedicated interactive CLI connection in `wrap`.
    let _permit = if transport.distro.is_empty() {
        Some(super::ssh_rpc_pool::acquire(&transport.host, transport.port, false).await?)
    } else {
        None
    };
    let mut command = base_ssh_command(transport);
    if transport.distro.is_empty() {
        command.arg(format!("umask 077; cat > {}", sh_quote(remote_path)));
    } else {
        command.arg(remote_command_string(transport, &["tee", remote_path]));
    }
    upload_child(command, script_body, REMOTE_CALL_TIMEOUT).await
}

/// Keep writes and diagnostic reads concurrent: SSH can fill stderr before
/// reading stdin, otherwise both pipes can block each other. All three phases
/// share one deadline, and diagnostics already received survive a timeout.
async fn upload_child(
    mut command: Command,
    script_body: &str,
    deadline: Duration,
) -> Result<(), String> {
    command
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .kill_on_drop(true);
    let mut child = command
        .spawn()
        .map_err(|e| format!("ssh 脚本上传失败: {e}"))?;
    let mut stdin = child
        .stdin
        .take()
        .ok_or("SSH upload stdin was unavailable")?;
    let stderr = child
        .stderr
        .take()
        .ok_or("SSH upload stderr was unavailable")?;
    let mut diagnostics = Vec::new();
    let mut write_finished = false;
    let mut ssh_exited = false;
    let completed = &mut write_finished;
    let writing = async move {
        use tokio::io::AsyncWriteExt;
        let result = stdin.write_all(script_body.as_bytes()).await;
        let _ = stdin.shutdown().await;
        // `shutdown` alone need not close a pipe. Own and drop the writer
        // here so the remote cat/tee receives EOF before we await SSH exit.
        drop(stdin);
        *completed = true;
        result
    };
    let result = tokio::time::timeout(deadline, async {
        tokio::join!(
            writing,
            drain_upload_stderr(stderr, &mut diagnostics),
            async {
                let result = child.wait().await;
                ssh_exited = true;
                result
            }
        )
    })
    .await;
    let (wrote, drained, waited) = match result {
        Ok(results) => results,
        Err(_) => {
            let _ = child.start_kill();
            let phase = if !write_finished {
                "writing SSH stdin"
            } else if !ssh_exited {
                "waiting for SSH exit"
            } else {
                "waiting for SSH stderr EOF"
            };
            let detail = String::from_utf8_lossy(&diagnostics);
            let detail = detail.trim();
            return Err(format!(
                "SSH script upload timed out ({}s; {phase}){}",
                deadline.as_secs(),
                if detail.is_empty() {
                    String::new()
                } else {
                    format!(": {detail}")
                }
            ));
        }
    };
    let status = waited.map_err(|e| format!("ssh 脚本上传等待失败: {e}"))?;
    if !status.success() {
        return Err(format!(
            "SSH script upload failed (exit {:?}): {}",
            status.code(),
            String::from_utf8_lossy(&diagnostics).trim()
        ));
    }
    wrote.map_err(|e| format!("ssh 脚本写入失败: {e}"))?;
    drained.map_err(|e| format!("SSH upload diagnostic read failed: {e}"))?;
    Ok(())
}

async fn drain_upload_stderr(
    mut stderr: tokio::process::ChildStderr,
    retained: &mut Vec<u8>,
) -> std::io::Result<()> {
    use tokio::io::AsyncReadExt;
    const DIAGNOSTIC_LIMIT: usize = 16 * 1024;
    let mut buffer = [0; 8192];
    loop {
        let count = stderr.read(&mut buffer).await?;
        if count == 0 {
            return Ok(());
        }
        let keep = count.min(DIAGNOSTIC_LIMIT.saturating_sub(retained.len()));
        retained.extend_from_slice(&buffer[..keep]);
    }
}

/// 包装结果:重造后的 ssh 命令 + 本地临时标记文件(run 结束清理)。
pub struct Wrapped {
    pub command: Command,
    pub cleanup_files: Vec<PathBuf>,
    /// 标记本次 spawn 不应设置本地 current_dir(远程路径在本机不存在)。
    pub skip_local_cwd: bool,
}

/// 把 build_command 产出的引擎命令包装成远程 ssh 执行。失败返回 Err。
pub async fn wrap(command: Command, transport: &WslTransport) -> Result<Wrapped, String> {
    let (program, args, envs) = program_args_env(&command);
    let script = build_script(&program, &args, &envs, transport);
    let script_id = uuid::Uuid::new_v4().simple().to_string();
    let remote_path = format!("/tmp/ccgui-wsl-{script_id}.sh");
    upload_script(transport, &script, &remote_path).await?;

    let local_tmp = std::env::temp_dir().join(format!("ccgui-wsl-{script_id}.marker"));
    std::fs::write(&local_tmp, b"").map_err(|e| format!("临时文件写入失败: {e}"))?;

    let mut wrapped = base_ssh_command(transport);
    // 脚本已明文落盘,run 串只含固定词。
    wrapped.arg(remote_command_string(transport, &["bash", &remote_path]));
    Ok(Wrapped {
        command: wrapped,
        cleanup_files: vec![local_tmp],
        skip_local_cwd: true,
    })
}

/// 供 send_message 判断工作区是否远程(直接读 db 行的 meta JSON)。
pub fn transport_from_meta_json(meta_json: Option<&str>) -> Option<WslTransport> {
    let raw = meta_json?;
    let value: serde_json::Value = serde_json::from_str(raw).ok()?;
    from_workspace_meta(&value)
}

/// 供调用方在 workspace meta 表里查询(路径 → meta 文本)。
pub fn workspace_meta_json(db: &crate::db::Db, workspace_path: &str) -> Option<String> {
    let conn = db.0.lock();
    conn.query_row(
        "SELECT meta FROM workspaces WHERE path = ?1",
        [workspace_path],
        |r| r.get::<_, Option<String>>(0),
    )
    .ok()
    .flatten()
}

/// 供 send_message 把本地 cwd 指到一个必然存在的目录(远程工作区场景)。
pub fn fallback_cwd() -> &'static std::path::Path {
    std::path::Path::new(".")
}

/// Run a bounded RPC with a dedicated Linux SSH connection, including writes.
/// Guaranteed read-only callers use `run_readonly_script_output` to reuse a
/// private connection. WSL retains its uploaded-script transport and noise
/// filtering under both APIs.
pub async fn run_script_output(
    transport: &WslTransport,
    script_body: &str,
) -> Result<String, String> {
    run_script_output_with_timeout(transport, script_body, REMOTE_CALL_TIMEOUT).await
}

/// Explicitly read-only calls may reuse a private, briefly persisted SSH
/// connection. Channel cancellation cannot promise to stop a remote mutation.
pub async fn run_readonly_script_output(
    transport: &WslTransport,
    script_body: &str,
) -> Result<String, String> {
    run_readonly_script_output_with_timeout(transport, script_body, REMOTE_CALL_TIMEOUT).await
}

/// Bulk transcript transfers have a separate deadline from small RPCs.
pub async fn run_script_output_with_timeout(
    transport: &WslTransport,
    script_body: &str,
    deadline: Duration,
) -> Result<String, String> {
    run_script_output_impl(transport, script_body, deadline, false).await
}

pub async fn run_readonly_script_output_with_timeout(
    transport: &WslTransport,
    script_body: &str,
    deadline: Duration,
) -> Result<String, String> {
    run_script_output_impl(transport, script_body, deadline, true).await
}

async fn run_script_output_impl(
    transport: &WslTransport,
    script_body: &str,
    deadline: Duration,
    readonly: bool,
) -> Result<String, String> {
    if transport.distro.is_empty() {
        // Serialize connection bursts across workspaces and blocking runtimes.
        // A bulk transfer reserves its own slot before competing for the
        // general pool, so queued downloads cannot occupy all short-RPC slots.
        let _permit = super::ssh_rpc_pool::acquire(
            &transport.host,
            transport.port,
            deadline > REMOTE_CALL_TIMEOUT,
        )
        .await?;
        return run_linux_script_output(transport, script_body, deadline, readonly).await;
    }
    let script_id = uuid::Uuid::new_v4().simple().to_string();
    let remote_path = format!("/tmp/ccgui-wsl-{script_id}.sh");
    // 调用方脚本同样落盘即删(bash 从已打开 fd 继续读,unlink 不影响执行)。
    let body = format!("rm -f \"$0\"\n{script_body}");
    upload_script(transport, &body, &remote_path).await?;
    let mut command = base_ssh_command(transport);
    command.arg(remote_command_string(transport, &["bash", &remote_path]));
    command
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    // 全局 deadline(理由见 REMOTE_CALL_TIMEOUT):模型目录/历史回放都走
    // 这里,远端挂起不得拖死前端。kill_on_drop:超时后 output future 被
    // 丢弃即 SIGKILL 本地 ssh(远端脚本落盘即删,无残留面)。
    command.kill_on_drop(true);
    let output = match tokio::time::timeout(deadline, command.output()).await {
        Ok(result) => result.map_err(|e| format!("远程命令执行失败: {e}"))?,
        Err(_) => {
            return Err(format!(
                "远程命令超时({}s,检查远程主机 wsl.exe 是否无响应)",
                deadline.as_secs()
            ));
        }
    };
    if !output.status.success() {
        return Err(format!(
            "远程命令退出码 {:?}: {}",
            output.status.code(),
            String::from_utf8_lossy(&output.stderr).trim()
        ));
    }
    if transport.distro.is_empty() {
        Ok(String::from_utf8_lossy(&output.stdout).into_owned())
    } else {
        Ok(String::from_utf8_lossy(&strip_transport_noise(output.stdout.as_slice())).to_string())
    }
}

/// Short Linux RPCs own stdin, so stream their script directly to one SSH
/// session. Engine runs still upload first to reserve stdin for chat/control.
async fn run_linux_script_output(
    transport: &WslTransport,
    script: &str,
    deadline: Duration,
    readonly: bool,
) -> Result<String, String> {
    let mut command = if readonly {
        readonly_ssh_command(transport)?
    } else {
        base_ssh_command(transport)
    };
    command.arg(remote_command_string(transport, &["bash", "-ls"]));
    run_linux_script_command(command, script, deadline).await
}

/// First-connect diagnostics must also work when Bash/Python are missing.
/// This fixed POSIX bootstrap prefers the same Bash login environment as
/// engine runs, but can report missing dependencies through a plain sh.
pub(super) async fn run_readonly_probe_output(
    transport: &WslTransport,
    script: &str,
) -> Result<String, String> {
    if !transport.distro.is_empty() {
        return Err("SSH dependency diagnostics require a Linux workspace".into());
    }
    let _permit = super::ssh_rpc_pool::acquire(&transport.host, transport.port, false).await?;
    let mut command = readonly_ssh_command(transport)?;
    command.arg(
        "sh -lc 'if command -v bash >/dev/null 2>&1; then exec bash -ls; else exec sh -ls; fi'",
    );
    run_linux_script_command(command, script, REMOTE_CALL_TIMEOUT).await
}

async fn run_linux_script_command(
    mut command: Command,
    script: &str,
    deadline: Duration,
) -> Result<String, String> {
    use tokio::io::AsyncWriteExt;
    command
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);
    let mut child = command
        .spawn()
        .map_err(|e| format!("SSH command failed to start: {e}"))?;
    let mut stdin = child.stdin.take().ok_or("SSH stdin was unavailable")?;
    let result = tokio::time::timeout(deadline, async {
        tokio::join!(
            async move {
                stdin.write_all(script.as_bytes()).await?;
                let result = stdin.shutdown().await;
                drop(stdin);
                result
            },
            child.wait_with_output()
        )
    })
    .await
    .map_err(|_| format!("Remote command timed out ({}s)", deadline.as_secs()))?;
    let (write_result, output) = result;
    let output = output.map_err(|e| format!("SSH command failed: {e}"))?;
    if !output.status.success() {
        return Err(format!(
            "Remote command exited {:?}: {}",
            output.status.code(),
            String::from_utf8_lossy(&output.stderr).trim()
        ));
    }
    write_result.map_err(|e| format!("SSH script write failed: {e}"))?;
    Ok(String::from_utf8_lossy(&output.stdout).into_owned())
}

/// Synchronous file/Git IPC handlers can use the same bounded transport.
/// A dedicated runtime thread also works when called from a Tokio worker.
pub fn run_script_output_blocking(
    transport: &WslTransport,
    script_body: &str,
) -> Result<String, String> {
    run_script_output_blocking_impl(transport, script_body, false)
}

pub fn run_readonly_script_output_blocking(
    transport: &WslTransport,
    script_body: &str,
) -> Result<String, String> {
    run_script_output_blocking_impl(transport, script_body, true)
}

fn run_script_output_blocking_impl(
    transport: &WslTransport,
    script_body: &str,
    readonly: bool,
) -> Result<String, String> {
    let transport = transport.clone();
    let body = script_body.to_string();
    std::thread::spawn(move || {
        tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .map_err(|e| e.to_string())?
            .block_on(run_script_output_impl(
                &transport,
                &body,
                REMOTE_CALL_TIMEOUT,
                readonly,
            ))
    })
    .join()
    .map_err(|_| "Remote command worker failed".to_string())?
}

/// wsl.exe 传输层噪声:NUL 残迹(UTF-16LE lossy 后)与 `wsl:`/`wsl.` 开头
/// 的诊断行。行结构保留(多行输出各剥各的),载荷行原样通过。
fn strip_transport_noise(raw: &[u8]) -> Vec<u8> {
    let text = String::from_utf8_lossy(raw).replace('\0', "");
    text.lines()
        .filter(|l| {
            let t = l.trim_start();
            !(t.starts_with("wsl:") || t.starts_with("wsl."))
        })
        .map(|l| format!("{l}\n"))
        .collect::<Vec<_>>()
        .join("")
        .into_bytes()
}

/// 工作区路径 → 传输描述一步到位(db 读 + meta 形状解析都收敛在此,
/// 调用方不再自行组合两步查找)。
pub fn transport_for_workspace(db: &crate::db::Db, workspace_path: &str) -> Option<WslTransport> {
    transport_for_path(db, workspace_path).ok().flatten()
}

/// Longest registered ancestor wins. Invalid matching metadata is an error,
/// so a corrupt remote row can never grant access to a similarly named Mac path.
pub fn transport_for_path(db: &crate::db::Db, path: &str) -> Result<Option<WslTransport>, String> {
    if path.contains('\0') || path.split('/').any(|part| part == "..") {
        return Err("Invalid workspace path".into());
    }
    let conn = db.0.lock();
    let mut stmt = conn
        .prepare("SELECT path, meta FROM workspaces ORDER BY length(path) DESC")
        .map_err(|e| e.to_string())?;
    let rows = stmt
        .query_map([], |r| {
            Ok((r.get::<_, String>(0)?, r.get::<_, Option<String>>(1)?))
        })
        .map_err(|e| e.to_string())?;
    for row in rows {
        let (root, meta) = row.map_err(|e| e.to_string())?;
        let root = root.trim_end_matches('/');
        if path == root
            || path
                .strip_prefix(root)
                .is_some_and(|tail| tail.starts_with('/'))
        {
            let Some(raw) = meta else {
                return Ok(None);
            };
            let value = serde_json::from_str(&raw)
                .map_err(|e| format!("Invalid workspace metadata: {e}"))?;
            return from_workspace_meta_checked(&value);
        }
    }
    Ok(None)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn tp() -> WslTransport {
        WslTransport {
            host: "10.0.0.2".into(),
            port: 22,
            user: "dev".into(),
            distro: "Ubuntu-22.04".into(),
            control_path: None,
            identity_file: None,
            engine_paths: HashMap::from([(
                "omp".to_string(),
                "/home/dev/.local/bin/omp".to_string(),
            )]),
            workspace: Some("/home/dev/proj".into()),
        }
    }

    #[test]
    fn meta_parses_engine_paths_and_workspace() {
        let meta = json!({"wsl": {
            "host": "10.0.0.2", "port": 2222, "user": "dev", "distro": "Ubuntu",
            "controlPath": "/tmp/m", "workspace": "/home/dev/p",
            "enginePaths": {"omp": "/home/dev/.local/bin/omp"}
        }});
        let t = from_workspace_meta(&meta).unwrap();
        assert_eq!(t.port, 2222);
        assert_eq!(t.control_path.as_deref(), Some("/tmp/m"));
        assert_eq!(
            t.engine_paths.get("omp").unwrap(),
            "/home/dev/.local/bin/omp"
        );
        assert_eq!(t.workspace.as_deref(), Some("/home/dev/p"));
    }

    #[test]
    fn script_maps_bin_cd_and_quotes_args() {
        let script = build_script(
            "/Users/x/.local/bin/omp",
            &[
                "-p".into(),
                "hello world".into(),
                "--resume".into(),
                "s-1".into(),
            ],
            &[],
            &tp(),
        );
        assert!(script.starts_with("set -e\n"));
        assert!(script.contains("cd /home/dev/proj || exit 61"));
        // argv[0] basename "omp" 命中 enginePaths → 发行版内路径
        assert!(script.contains("exec '/home/dev/.local/bin/omp'"));
        assert!(script.contains("'hello world'"));
        // 本机 mac 路径绝不残留
        assert!(!script.contains("/Users/"));
    }

    #[test]
    fn script_keeps_tilde_unquoted_for_native_expansion() {
        let mut t = tp();
        t.workspace = Some("~/code/proj".into());
        let script = build_script("omp", &[], &[], &t);
        assert!(script.contains("cd ~/code/proj || exit 61"));
        // 引号包裹会杀死 bash 的 tilde 展开 —— 绝不能出现
        assert!(!script.contains("\"~/"));
    }

    #[test]
    fn script_rejects_space_paths() {
        let mut t = tp();
        t.workspace = Some("/home/dev/my proj".into());
        let script = build_script("omp", &[], &[], &t);
        assert!(script.contains("exit 60"));
    }

    #[test]
    fn script_falls_back_to_command_v() {
        let script = build_script("kimi", &[], &[], &tp());
        assert!(script.contains("command -v"));
        assert!(script.contains("exit 62"));
        assert!(script.contains("exec \"$bin\""));
    }

    #[test]
    fn remote_command_string_is_shell_inert() {
        let s = wsl_command_string(&tp(), &["bash", "/tmp/x.sh"]);
        assert_eq!(s, "wsl.exe -d \"Ubuntu-22.04\" -- \"bash\" \"/tmp/x.sh\"");
        assert!(!s.contains('$') && !s.contains('|') && !s.contains('`'));
    }

    #[test]
    fn meta_rejects_ssh_option_injection_user() {
        // user 以 `-` 开头:`user@host` 会被 ssh getopt 吞成 -o 选项
        // (ProxyCommand → 本机命令执行),必须整个拒绝。
        let meta = json!({"wsl": {
            "host": "10.0.0.2", "user": "-oProxyCommand=/tmp/evil@x", "distro": "Ubuntu"
        }});
        assert!(from_workspace_meta(&meta).is_none());
    }

    #[test]
    fn meta_rejects_dash_host_and_bad_chars() {
        for host in ["-oProxyCommand=x", "ho st", "ho;st", "ho`id`st"] {
            let meta = json!({"wsl": { "host": host, "user": "dev", "distro": "Ubuntu" }});
            assert!(from_workspace_meta(&meta).is_none(), "host: {host}");
        }
        // IPv6 / 主机名 / 带点用户名为合法形态
        let meta = json!({"wsl": {
            "host": "::1", "user": "dev.ops-1_x", "distro": "Ubuntu"
        }});
        assert!(from_workspace_meta(&meta).is_some());
    }

    #[test]
    fn meta_rejects_distro_shell_chars() {
        // PowerShell 双引号内 $(…)/反引号仍求值,cmd 下 %VAR% 仍展开
        for distro in ["Ubuntu$(id)", "u`id`", "u%PATH%", "u\"&id", "u|id"] {
            let meta = json!({"wsl": { "host": "10.0.0.2", "user": "dev", "distro": distro }});
            assert!(from_workspace_meta(&meta).is_none(), "distro: {distro}");
        }
        let meta = json!({"wsl": {
            "host": "10.0.0.2", "user": "dev", "distro": "Ubuntu-22.04 LTS"
        }});
        assert!(from_workspace_meta(&meta).is_some());
    }

    #[test]
    fn workspace_allowlist() {
        assert!(is_safe_workspace("/home/dev/proj"));
        assert!(is_safe_workspace("~/code/proj"));
        assert!(!is_safe_workspace("/home/dev/my proj"));
        assert!(!is_safe_workspace("/a;id"));
        assert!(!is_safe_workspace("/a$(id)"));
        assert!(!is_safe_workspace(""));
    }

    #[test]
    fn script_exports_env_and_self_deletes() {
        let envs = vec![("MAX_THINKING_TOKENS".to_string(), "65536".to_string())];
        let script = build_script("omp", &[], &envs, &tp());
        // 落盘即删(exec 下 trap 不触发,unlink 已打开 fd 的脚本仍然安全)
        assert!(script.contains("rm -f \"$0\""));
        assert!(script.contains("export MAX_THINKING_TOKENS='65536'"));
        // 值含单引号/空白也走 sh_quote,不直接拼接
        let envs = vec![("X_Y".to_string(), "a'b c".to_string())];
        let script = build_script("omp", &[], &envs, &tp());
        assert!(script.contains("export X_Y='a'\\''b c'"));
    }

    #[test]
    fn ssh_command_separates_destination_with_double_dash() {
        let cmd = base_ssh_command(&tp());
        let args: Vec<String> = cmd
            .as_std()
            .get_args()
            .map(|a| a.to_string_lossy().into_owned())
            .collect();
        let dashdash = args.iter().position(|a| a == "--").expect("missing --");
        // `--` 之后紧跟 destination,且 destination 之前没有任何非选项值
        assert_eq!(args[dashdash + 1], "dev@10.0.0.2");
        assert!(args[..dashdash]
            .iter()
            .all(|a| a.starts_with('-') || !a.contains('@')));
    }

    #[test]
    fn readonly_mux_is_explicit_and_dedicated_connections_never_inherit_it() {
        let mut transport = tp();
        transport.distro.clear();
        // A Linux row cannot inject an alternative control socket ahead of
        // the dedicated/read-only mode (OpenSSH's first value wins).
        transport.control_path = Some("/tmp/untrusted-control".into());
        let args = |command: Command| {
            command
                .as_std()
                .get_args()
                .map(|arg| arg.to_string_lossy().into_owned())
                .collect::<Vec<_>>()
        };
        let dedicated = args(base_ssh_command(&transport));
        assert!(dedicated.contains(&"ControlMaster=no".into()));
        assert!(dedicated.contains(&"ControlPath=none".into()));
        assert!(!dedicated
            .iter()
            .any(|arg| arg.starts_with("ControlPersist=")));
        let readonly = args(ssh_command_with_control(
            &transport,
            Some(std::path::Path::new("/private/tmp/private.sock")),
        ));
        assert!(readonly.contains(&"ControlMaster=auto".into()));
        assert!(readonly.contains(&"ControlPersist=60".into()));
        assert!(readonly.contains(&"ControlPath=/private/tmp/private.sock".into()));
        let separator = readonly.iter().position(|arg| arg == "--").unwrap();
        assert_eq!(readonly[separator + 1], "dev@10.0.0.2");
        for option in [
            "ControlMaster=auto",
            "ControlPersist=60",
            "ControlPath=/private/tmp/private.sock",
        ] {
            assert!(readonly[..separator].contains(&option.into()));
        }
        for option in ["ControlMaster=", "ControlPath="] {
            assert_eq!(
                readonly
                    .iter()
                    .filter(|arg| arg.starts_with(option))
                    .count(),
                1
            );
            assert_eq!(
                dedicated
                    .iter()
                    .filter(|arg| arg.starts_with(option))
                    .count(),
                1
            );
        }
        for option in [
            "ForwardAgent=no",
            "ForwardX11=no",
            "ClearAllForwardings=yes",
            "RequestTTY=no",
        ] {
            assert!(readonly[..separator].contains(&option.into()));
        }
        assert!(!readonly.iter().any(|arg| arg.contains("untrusted-control")));
        let wsl = args(ssh_command_with_control(
            &tp(),
            Some(std::path::Path::new("/tmp/ignored")),
        ));
        assert!(!wsl
            .iter()
            .any(|arg| arg.starts_with("ControlMaster=") || arg.starts_with("ControlPersist=")));
    }

    #[test]
    fn linux_metadata_uses_key_and_strict_known_hosts() {
        let value = json!({"ssh": {"host":"vps.example","user":"dev","port":2222,
            "identityFile":"/Users/dev/.ssh/vps","workspace":"/srv/project with spaces",
            "enginePaths":{"claude":"/home/dev/.local/bin/claude"}}});
        let transport = from_workspace_meta_checked(&value).unwrap().unwrap();
        assert!(transport.distro.is_empty());
        let opts = ssh_options(&transport);
        assert!(opts.contains(&"StrictHostKeyChecking=yes".into()));
        assert!(!opts.contains(&"StrictHostKeyChecking=accept-new".into()));
        assert!(opts.contains(&"/Users/dev/.ssh/vps".into()));
        assert!(opts.contains(&"IdentitiesOnly=yes".into()));
        let script = build_script(
            "claude",
            &["--model".into(), "x'; touch /tmp/evil".into()],
            &[],
            &transport,
        );
        assert!(script.contains("cd -- '/srv/project with spaces'"));
        assert!(script.contains("setsid bash -c"));
        assert!(script.contains("<&0 &"));
        assert!(script.contains("parent=$PPID"));
        assert!(!script.contains("wsl.exe"));
    }

    #[test]
    fn linux_ssh_compresses_bulk_scripts_without_overriding_qos_or_wsl() {
        let mut transport = tp();
        assert!(!ssh_options(&transport)
            .iter()
            .any(|arg| arg.starts_with("Compression=")));
        transport.distro.clear();
        let opts = ssh_options(&transport);
        assert!(opts
            .windows(2)
            .any(|pair| pair == ["-o", "Compression=yes"]));
        assert!(!opts.iter().any(|arg| arg.starts_with("IPQoS=")));
    }

    #[test]
    fn malformed_remote_metadata_is_an_error() {
        for value in [
            json!({"ssh":{"host":"-evil"}}),
            json!({"ssh":{"host":"vps","user":"dev","workspace":"/srv/../etc"}}),
            json!({"ssh":{"host":"vps","user":"dev","workspace":"/srv","port":0}}),
        ] {
            assert!(from_workspace_meta_checked(&value).is_err());
        }
        assert!(from_workspace_meta_checked(&json!({"other":true}))
            .unwrap()
            .is_none());
    }

    #[test]
    fn lookup_uses_registered_ancestor_and_refuses_corrupt_meta() {
        let connection = rusqlite::Connection::open_in_memory().unwrap();
        connection
            .execute_batch("CREATE TABLE workspaces(path TEXT, meta TEXT)")
            .unwrap();
        connection
            .execute(
                "INSERT INTO workspaces VALUES(?1,?2)",
                rusqlite::params![
                    "/srv/proj",
                    json!({"ssh":{"host":"vps","user":"dev","workspace":"/srv/proj"}}).to_string()
                ],
            )
            .unwrap();
        connection
            .execute("INSERT INTO workspaces VALUES('/srv/broken','{')", [])
            .unwrap();
        let db = crate::db::Db(parking_lot::Mutex::new(connection));
        assert_eq!(
            transport_for_path(&db, "/srv/proj/src/main.rs")
                .unwrap()
                .unwrap()
                .workspace
                .as_deref(),
            Some("/srv/proj")
        );
        assert!(transport_for_path(&db, "/srv/project").unwrap().is_none());
        assert!(transport_for_path(&db, "/srv/proj/../etc").is_err());
        assert!(transport_for_path(&db, "/srv/broken/file").is_err());
    }

    #[cfg(unix)]
    fn upload_fixture(script: &str) -> Command {
        let mut command = Command::new("python3");
        command.args(["-u", "-c", script]);
        command
    }

    #[cfg(unix)]
    struct UploadPidFile(PathBuf);

    #[cfg(unix)]
    impl UploadPidFile {
        fn new() -> Self {
            Self(std::env::temp_dir().join(format!(
                "ccgui-upload-{}.pid",
                uuid::Uuid::new_v4().simple()
            )))
        }

        async fn pid(&self) -> u32 {
            tokio::time::timeout(Duration::from_secs(3), async {
                loop {
                    if let Ok(text) = std::fs::read_to_string(&self.0) {
                        if let Ok(pid) = text.trim().parse() {
                            return pid;
                        }
                    }
                    tokio::time::sleep(Duration::from_millis(10)).await;
                }
            })
            .await
            .expect("upload fixture did not start")
        }
    }

    #[cfg(unix)]
    impl Drop for UploadPidFile {
        fn drop(&mut self) {
            let _ = std::fs::remove_file(&self.0);
        }
    }

    #[cfg(unix)]
    async fn assert_upload_fixture_exited(pid: u32) {
        tokio::time::timeout(Duration::from_secs(3), async {
            while unsafe { libc::kill(pid as libc::pid_t, 0) } == 0 {
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .expect("upload child survived timeout/cancellation");
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn upload_drains_stderr_before_child_reads_large_stdin() {
        let body = "x".repeat(1024 * 1024);
        let command = upload_fixture(
            "import sys; sys.stderr.write('diagnostic-'*100000); sys.stderr.flush(); body=sys.stdin.buffer.read(); sys.exit(0 if len(body)==1048576 else 4)"
        );
        upload_child(command, &body, Duration::from_secs(5))
            .await
            .unwrap();
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn upload_timeout_keeps_bounded_diagnostics_and_kills_blocked_writer() {
        let pid_file = UploadPidFile::new();
        let mut command = upload_fixture(
            "import os,sys,time; open(sys.argv[1],'w').write(str(os.getpid())); sys.stderr.write('upload-stalled\\n'+'x'*1048576); sys.stderr.flush(); time.sleep(10)"
        );
        command.arg(&pid_file.0);
        let error = upload_child(
            command,
            &"x".repeat(1024 * 1024),
            Duration::from_millis(300),
        )
        .await
        .unwrap_err();
        assert!(error.contains("writing SSH stdin"), "{error}");
        assert!(error.contains("upload-stalled"));
        assert!(error.len() < 17 * 1024, "diagnostics must remain bounded");
        assert_upload_fixture_exited(pid_file.pid().await).await;
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn upload_write_and_wait_share_one_deadline_and_keep_exit_diagnostic() {
        let command = upload_fixture(
            "import sys,time; sys.stderr.write('waiting-exit\\n'); sys.stderr.flush(); time.sleep(.2); sys.stdin.buffer.read(); time.sleep(.2)"
        );
        let error = upload_child(
            command,
            &"x".repeat(1024 * 1024),
            Duration::from_millis(300),
        )
        .await
        .unwrap_err();
        assert!(error.contains("waiting for SSH exit"), "{error}");
        assert!(error.contains("waiting-exit"));
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn upload_cancellation_kills_only_its_owned_child() {
        let pid_file = UploadPidFile::new();
        let mut command = upload_fixture(
            "import os,sys,time; open(sys.argv[1],'w').write(str(os.getpid())); time.sleep(10)",
        );
        command.arg(&pid_file.0);
        let task = tokio::spawn(async move {
            upload_child(command, &"x".repeat(1024 * 1024), Duration::from_secs(30)).await
        });
        let pid = pid_file.pid().await;
        task.abort();
        assert!(task.await.unwrap_err().is_cancelled());
        assert_upload_fixture_exited(pid).await;
    }

    /// Opt-in against a disposable directory on a configured Linux host.
    /// This runs Python/sleep only and never sends a model request.
    #[tokio::test]
    #[ignore = "requires CCGUI_SSH_TEST_HOST, CCGUI_SSH_TEST_USER and optional CCGUI_SSH_TEST_KEY"]
    async fn ssh_supervisor_stops_group_and_preserves_stdin() {
        use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
        let transport = WslTransport {
            host: std::env::var("CCGUI_SSH_TEST_HOST").expect("test host"),
            user: std::env::var("CCGUI_SSH_TEST_USER").expect("test user"),
            port: 22,
            distro: String::new(),
            identity_file: std::env::var("CCGUI_SSH_TEST_KEY").ok(),
            control_path: None,
            engine_paths: HashMap::new(),
            workspace: Some("/tmp".into()),
        };
        let mut command = Command::new("python3");
        command.args(["-c", "import os,sys,time,subprocess; child=subprocess.Popen(['sleep','120']); print(str(os.getpid())+' '+str(child.pid),flush=True); print(sys.stdin.readline().strip(),flush=True); time.sleep(120)"]);
        let mut wrapped = wrap(command, &transport).await.unwrap();
        wrapped
            .command
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        let mut child = wrapped.command.spawn().unwrap();
        let mut stdin = child.stdin.take().unwrap();
        stdin.write_all(b"permission-answer\n").await.unwrap();
        stdin.flush().await.unwrap();
        let mut stdout = BufReader::new(child.stdout.take().unwrap());
        let mut pids = String::new();
        tokio::time::timeout(Duration::from_secs(15), stdout.read_line(&mut pids))
            .await
            .unwrap()
            .unwrap();
        let pids: Vec<u32> = pids
            .split_whitespace()
            .map(|pid| pid.parse().unwrap())
            .collect();
        assert_eq!(pids.len(), 2);
        let mut answer = String::new();
        tokio::time::timeout(Duration::from_secs(5), stdout.read_line(&mut answer))
            .await
            .unwrap()
            .unwrap();
        assert_eq!(answer.trim(), "permission-answer");
        child.start_kill().unwrap();
        child.wait().await.unwrap();
        let check = format!("sleep 5; ps -o stat= -p {},{} || true", pids[0], pids[1]);
        let states = run_script_output(&transport, &check).await.unwrap();
        // A zombie has exited and cannot run tools; init will reap it.
        assert!(
            states.lines().all(|state| state.trim().starts_with('Z')),
            "remote processes survived: {states}"
        );
        for file in wrapped.cleanup_files {
            let _ = std::fs::remove_file(file);
        }
    }

    /// Read-only marker commands only. This fixture owns a previously unused
    /// private socket and closes that master; it does not touch the live CLI.
    #[cfg(unix)]
    #[tokio::test]
    #[ignore = "requires CCGUI_SSH_LIVE_HOST/USER/KEY and an unused read-only control socket"]
    async fn ssh_readonly_mux_cold_and_warm_batches() {
        use std::sync::Arc;
        use std::time::Instant;
        let transport = WslTransport {
            host: std::env::var("CCGUI_SSH_LIVE_HOST").expect("test host"),
            user: std::env::var("CCGUI_SSH_LIVE_USER").expect("test user"),
            port: 22,
            distro: String::new(),
            identity_file: std::env::var("CCGUI_SSH_LIVE_KEY").ok(),
            control_path: None,
            engine_paths: HashMap::new(),
            workspace: Some("/tmp".into()),
        };
        let path = super::super::ssh_rpc_mux::control_path(
            &transport.host,
            &transport.user,
            transport.port,
            transport.identity_file.as_deref(),
        )
        .unwrap()
        .unwrap();
        assert!(
            std::fs::symlink_metadata(&path).is_err(),
            "test will not take over an existing master"
        );
        struct Cleanup {
            transport: WslTransport,
            path: PathBuf,
            active: bool,
        }
        impl Drop for Cleanup {
            fn drop(&mut self) {
                if self.active {
                    let _ = std::process::Command::new("ssh")
                        .args(ssh_options(&self.transport))
                        .args([
                            "-S",
                            self.path.to_str().unwrap(),
                            "-O",
                            "exit",
                            "--",
                            &ssh_target(&self.transport),
                        ])
                        .stdin(Stdio::null())
                        .stdout(Stdio::null())
                        .stderr(Stdio::null())
                        .status();
                }
            }
        }
        let mut cleanup = Cleanup {
            transport: transport.clone(),
            path: path.clone(),
            active: true,
        };
        let transport = Arc::new(transport);
        for (label, count) in [("cold", 3), ("warm", 10)] {
            let started = Instant::now();
            let mut tasks = Vec::new();
            for index in 0..count {
                let transport = Arc::clone(&transport);
                tasks.push(tokio::spawn(async move {
                    let marker = format!("CCGUI_READONLY_{label}_{index}");
                    let script = format!("printf '%s' {}", sh_quote(&marker));
                    let started = Instant::now();
                    let output = run_readonly_script_output(&transport, &script)
                        .await
                        .unwrap();
                    assert_eq!(output, marker);
                    started.elapsed().as_millis()
                }));
            }
            let mut durations = Vec::new();
            for task in tasks {
                durations.push(task.await.unwrap());
            }
            eprintln!(
                "readonly {label} batch: elapsed_ms={}, rpc_ms={durations:?}",
                started.elapsed().as_millis()
            );
        }
        let control = |operation: &str| {
            let mut command = Command::new("ssh");
            command.args(ssh_options(&transport));
            command.args([
                "-S",
                path.to_str().unwrap(),
                "-O",
                operation,
                "--",
                &ssh_target(&transport),
            ]);
            command
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .kill_on_drop(true);
            command
        };
        assert!(
            tokio::time::timeout(Duration::from_secs(5), control("check").status())
                .await
                .unwrap()
                .unwrap()
                .success()
        );
        assert!(
            tokio::time::timeout(Duration::from_secs(5), control("exit").status())
                .await
                .unwrap()
                .unwrap()
                .success()
        );
        cleanup.active = false;
        eprintln!("readonly test master checked and closed");
    }
}
