//! Explicit first-connect host trust. Scanning does not authenticate a host:
//! callers must verify the displayed SHA256 fingerprint out of band before
//! requesting trust. Changed/revoked keys are never replaced or appended.

use super::wsl_transport::{is_safe_host, is_safe_user};
use base64::{engine::general_purpose::STANDARD, Engine as _};
use serde::Serialize;
use sha2::{Digest, Sha256};
use std::fs::{File, OpenOptions};
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::process::{ExitStatus, Stdio};
use std::sync::LazyLock;
use std::time::Duration;
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWriteExt};
use tokio::process::Command;

const TOOL_TIMEOUT: Duration = Duration::from_secs(12);
const FILE_LIMIT: u64 = 4 * 1024 * 1024;
const OUTPUT_LIMIT: usize = 128 * 1024;
static TRUST_LOCK: LazyLock<tokio::sync::Mutex<()>> = LazyLock::new(|| tokio::sync::Mutex::new(()));
static SCAN_LIMIT: LazyLock<tokio::sync::Semaphore> =
    LazyLock::new(|| tokio::sync::Semaphore::new(1));

#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct SshHostKey {
    pub algorithm: String,
    pub fingerprint: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SshHostKeyInspection {
    pub status: String,
    pub keys: Vec<SshHostKey>,
    pub known_fingerprints: Vec<String>,
}

#[derive(Clone, Debug)]
struct Key {
    algorithm: String,
    encoded: String,
    fingerprint: String,
    revoked: bool,
}

struct HostConfig {
    lookup: String,
    user_files: Vec<PathBuf>,
    global_files: Vec<PathBuf>,
    hash_new_hosts: bool,
}

fn endpoint(host: &str, port: u16, user: Option<&str>) -> Result<String, String> {
    let host = host.trim().to_ascii_lowercase();
    if host.len() > 253 || !is_safe_host(&host) || port == 0 {
        return Err("Invalid SSH host or port".into());
    }
    if user.is_some_and(|user| !is_safe_user(user.trim())) {
        return Err("Invalid SSH user".into());
    }
    Ok(host)
}

fn tool(name: &str) -> Command {
    // Use the operating system's OpenSSH rather than a project executable.
    #[cfg(unix)]
    let path = Path::new("/usr/bin").join(name);
    #[cfg(not(unix))]
    let path = PathBuf::from(name);
    Command::new(path)
}

async fn capture<R: AsyncRead + Unpin>(mut reader: R, limit: usize) -> Result<Vec<u8>, String> {
    let mut data = Vec::new();
    let mut buffer = [0u8; 4096];
    loop {
        let count = reader
            .read(&mut buffer)
            .await
            .map_err(|_| "OpenSSH output could not be read")?;
        if count == 0 {
            return Ok(data);
        }
        if data.len() + count > limit {
            return Err("OpenSSH diagnostic output exceeded its limit".into());
        }
        data.extend_from_slice(&buffer[..count]);
    }
}

async fn run_tool(
    mut command: Command,
    input: Option<&[u8]>,
    deadline: Duration,
) -> Result<(ExitStatus, Vec<u8>), String> {
    command
        .stdin(if input.is_some() {
            Stdio::piped()
        } else {
            Stdio::null()
        })
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);
    let mut child = command
        .spawn()
        .map_err(|_| "Required system OpenSSH tool could not be started")?;
    let stdin = child.stdin.take();
    let stdout = child
        .stdout
        .take()
        .ok_or("OpenSSH stdout was unavailable")?;
    let stderr = child
        .stderr
        .take()
        .ok_or("OpenSSH stderr was unavailable")?;
    tokio::time::timeout(deadline, async {
        let write = async move {
            if let (Some(mut stdin), Some(input)) = (stdin, input) {
                stdin
                    .write_all(input)
                    .await
                    .map_err(|_| "OpenSSH input could not be written")?;
                stdin
                    .shutdown()
                    .await
                    .map_err(|_| "OpenSSH input could not be closed")?;
                drop(stdin);
            }
            Ok::<_, String>(())
        };
        let ((), stdout, _, status) = tokio::try_join!(
            write,
            capture(stdout, OUTPUT_LIMIT),
            capture(stderr, 16 * 1024),
            async {
                child
                    .wait()
                    .await
                    .map_err(|_| "OpenSSH tool could not be reaped".to_string())
            }
        )?;
        Ok((status, stdout))
    })
    .await
    .map_err(|_| "OpenSSH host-key check timed out (12s)".to_string())?
}

fn known_files(value: &str, home: &Path) -> Result<Vec<PathBuf>, String> {
    if value == "none" {
        return Ok(Vec::new());
    }
    let files = value
        .split_whitespace()
        .map(|value| {
            let path = if let Some(rest) = value.strip_prefix("~/") {
                home.join(rest)
            } else {
                PathBuf::from(value)
            };
            if !path.is_absolute() || value.contains(['%', '\0', '\n', '\r', '"']) {
                return Err("Custom known_hosts path requires manual OpenSSH setup".to_string());
            }
            Ok(path)
        })
        .collect::<Result<Vec<_>, _>>()?;
    if files.len() > 8 {
        return Err("Too many OpenSSH known_hosts files".into());
    }
    Ok(files)
}

fn parse_config(output: &[u8], host: &str, port: u16, home: &Path) -> Result<HostConfig, String> {
    let text = std::str::from_utf8(output).map_err(|_| "Invalid OpenSSH configuration output")?;
    let fields: std::collections::HashMap<_, _> = text
        .lines()
        .filter_map(|line| line.split_once(' '))
        .collect();
    let hostname = fields
        .get("hostname")
        .copied()
        .ok_or("OpenSSH did not resolve a host name")?;
    // keyscan cannot reproduce arbitrary SSH proxy/routing commands. Refuse
    // automatic trust rather than scan a different endpoint than the app.
    if !hostname.eq_ignore_ascii_case(host)
        || fields.get("proxycommand").is_some_and(|v| *v != "none")
        || fields.get("proxyjump").is_some_and(|v| *v != "none")
        || fields
            .get("knownhostscommand")
            .is_some_and(|v| *v != "none")
        || fields.get("revokedhostkeys").is_some_and(|v| *v != "none")
        || fields
            .get("canonicalizehostname")
            .is_some_and(|v| !matches!(*v, "false" | "no"))
    {
        return Err(
            "This SSH routing or host-key configuration requires manual OpenSSH trust setup".into(),
        );
    }
    let lookup = match fields
        .get("hostkeyalias")
        .copied()
        .filter(|alias| *alias != "none")
    {
        Some(alias) if alias.len() <= 253 && is_safe_host(alias) => alias.to_string(),
        Some(_) => return Err("Invalid SSH host-key alias".into()),
        None if port == 22 => host.to_string(),
        None => format!("[{host}]:{port}"),
    };
    let user_files = known_files(
        fields.get("userknownhostsfile").copied().unwrap_or(""),
        home,
    )?;
    let global_files = known_files(
        fields.get("globalknownhostsfile").copied().unwrap_or(""),
        home,
    )?;
    Ok(HostConfig {
        lookup,
        user_files,
        global_files,
        hash_new_hosts: fields
            .get("hashknownhosts")
            .is_some_and(|v| matches!(*v, "true" | "yes")),
    })
}

async fn config(host: &str, port: u16, user: Option<&str>) -> Result<HostConfig, String> {
    let mut command = tool("ssh");
    command.args([
        "-G",
        "-o",
        "StrictHostKeyChecking=yes",
        "-o",
        "BatchMode=yes",
        "-o",
        "ControlMaster=no",
        "-o",
        "ControlPath=none",
        "-p",
        &port.to_string(),
    ]);
    if let Some(user) = user {
        command.args(["-l", user.trim()]);
    }
    command.args(["--", host]);
    let (status, output) = run_tool(command, None, TOOL_TIMEOUT).await?;
    if !status.success() {
        return Err("OpenSSH configuration could not be inspected".into());
    }
    parse_config(
        &output,
        host,
        port,
        &dirs::home_dir().ok_or("Home directory is unavailable")?,
    )
}

fn parse_keys(output: &[u8]) -> Result<Vec<Key>, String> {
    let text = std::str::from_utf8(output).map_err(|_| "Invalid host-key response")?;
    let mut keys = Vec::new();
    for line in text
        .lines()
        .filter(|line| !line.trim().is_empty() && !line.starts_with('#'))
    {
        let parts: Vec<_> = line.split_whitespace().collect();
        let (offset, revoked) = if parts.first().is_some_and(|v| v.starts_with('@')) {
            if parts[0] != "@revoked" {
                return Err(
                    "Certificate-authority host trust requires manual OpenSSH setup".into(),
                );
            }
            (1, true)
        } else {
            (0, false)
        };
        if parts.len() < offset + 3 {
            return Err("Invalid known_hosts key record".into());
        }
        let algorithm = parts[offset + 1];
        if !matches!(
            algorithm,
            "ssh-ed25519"
                | "ecdsa-sha2-nistp256"
                | "ecdsa-sha2-nistp384"
                | "ecdsa-sha2-nistp521"
                | "ssh-rsa"
        ) {
            return Err(
                "Unsupported host-key algorithm; manage this host with OpenSSH first".into(),
            );
        }
        let encoded = parts[offset + 2];
        if encoded.len() > 16 * 1024 {
            return Err("Host key exceeded its size limit".into());
        }
        let bytes = STANDARD
            .decode(encoded)
            .map_err(|_| "Invalid host-key encoding")?;
        let type_length = bytes
            .get(..4)
            .map(|value| u32::from_be_bytes(value.try_into().unwrap()) as usize)
            .ok_or("Invalid host-key wire format")?;
        if type_length > 64 || bytes.get(4..4 + type_length) != Some(algorithm.as_bytes()) {
            return Err("Host-key algorithm does not match its wire encoding".into());
        }
        let fingerprint = format!(
            "SHA256:{}",
            STANDARD
                .encode(Sha256::digest(&bytes))
                .trim_end_matches('=')
        );
        if !keys
            .iter()
            .any(|key: &Key| key.fingerprint == fingerprint && key.revoked == revoked)
        {
            keys.push(Key {
                algorithm: algorithm.into(),
                encoded: encoded.into(),
                fingerprint,
                revoked,
            });
        }
        if keys.len() > 64 {
            return Err("Too many host keys for this endpoint".into());
        }
    }
    Ok(keys)
}

struct Snapshot {
    directory: PathBuf,
    path: PathBuf,
}
impl Snapshot {
    fn new(bytes: &[u8]) -> Result<Self, String> {
        let directory =
            std::env::temp_dir().join(format!("ccgui-hostkeys-{}", uuid::Uuid::new_v4()));
        let mut builder = std::fs::DirBuilder::new();
        #[cfg(unix)]
        {
            use std::os::unix::fs::DirBuilderExt;
            builder.mode(0o700);
        }
        builder
            .create(&directory)
            .map_err(|_| "Could not create private host-key snapshot")?;
        let directory = directory
            .canonicalize()
            .map_err(|_| "Could not resolve private host-key snapshot")?;
        let result = Self {
            path: directory.join("keys"),
            directory,
        };
        let mut opts = OpenOptions::new();
        opts.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            opts.mode(0o600).custom_flags(libc::O_NOFOLLOW);
        }
        opts.open(&result.path)
            .and_then(|mut file| file.write_all(bytes))
            .map_err(|_| "Could not write private host-key snapshot")?;
        Ok(result)
    }
}
impl Drop for Snapshot {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.path);
        let _ = std::fs::remove_file(self.path.with_extension("old"));
        let _ = std::fs::remove_dir(&self.directory);
    }
}

async fn validate_keys(keys: &[Key]) -> Result<(), String> {
    let text: String = keys
        .iter()
        .map(|key| format!("host {} {}\n", key.algorithm, key.encoded))
        .collect();
    let snapshot = Snapshot::new(text.as_bytes())?;
    let mut command = tool("ssh-keygen");
    command
        .args(["-l", "-E", "sha256", "-f"])
        .arg(&snapshot.path);
    let (status, output) = run_tool(command, None, TOOL_TIMEOUT).await?;
    let fingerprints: Vec<_> = std::str::from_utf8(&output)
        .unwrap_or("")
        .lines()
        .filter_map(|line| line.split_whitespace().nth(1))
        .collect();
    if !status.success()
        || keys
            .iter()
            .any(|key| !fingerprints.contains(&key.fingerprint.as_str()))
    {
        return Err("OpenSSH rejected a scanned host key".into());
    }
    Ok(())
}

async fn scan(host: &str, port: u16) -> Result<Vec<Key>, String> {
    let _scan = tokio::time::timeout(TOOL_TIMEOUT, SCAN_LIMIT.acquire())
        .await
        .map_err(|_| "Host-key inspection is busy; try again")?
        .map_err(|_| "Host-key inspection is unavailable")?;
    let _pool = super::ssh_rpc_pool::acquire(host, port, false).await?;
    let mut command = tool("ssh-keyscan");
    command.args([
        "-T",
        "5",
        "-p",
        &port.to_string(),
        "-t",
        "ed25519,ecdsa,rsa",
        host,
    ]);
    let (status, output) = run_tool(command, None, TOOL_TIMEOUT).await?;
    if !status.success() {
        return Err("SSH host-key scan failed; check the host and port".into());
    }
    let keys = parse_keys(&output)?;
    if keys.is_empty() {
        return Err("SSH server did not return a supported host key".into());
    }
    validate_keys(&keys).await?;
    Ok(keys)
}

fn read_file(path: &Path) -> Result<Option<Vec<u8>>, String> {
    let mut opts = OpenOptions::new();
    opts.read(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        opts.custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK);
    }
    let mut file = match opts.open(path) {
        Ok(file) => file,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(_) => return Err("A known_hosts file could not be safely read".into()),
    };
    let meta = file
        .metadata()
        .map_err(|_| "Could not inspect known_hosts file")?;
    if !meta.is_file() || meta.len() > FILE_LIMIT {
        return Err("known_hosts must be a regular file under 4 MiB".into());
    }
    let mut bytes = Vec::new();
    Read::by_ref(&mut file)
        .take(FILE_LIMIT + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| "Could not read known_hosts file")?;
    if bytes.len() as u64 > FILE_LIMIT {
        return Err("known_hosts exceeded its size limit".into());
    }
    Ok(Some(bytes))
}

async fn lookup(config: &HostConfig) -> Result<Vec<Key>, String> {
    let mut keys = Vec::new();
    for path in config.user_files.iter().chain(&config.global_files) {
        let Some(bytes) = read_file(path)? else {
            continue;
        };
        let snapshot = Snapshot::new(&bytes)?;
        let mut command = tool("ssh-keygen");
        command
            .args(["-F", &config.lookup, "-f"])
            .arg(&snapshot.path);
        let (status, output) = run_tool(command, None, TOOL_TIMEOUT).await?;
        if status.code() == Some(1) && output.is_empty() {
            continue;
        }
        if !status.success() {
            return Err("OpenSSH could not inspect known_hosts entries".into());
        }
        keys.extend(parse_keys(&output)?);
    }
    Ok(keys)
}

fn inspection(scanned: &[Key], known: &[Key]) -> SshHostKeyInspection {
    let matches = |key: &Key| {
        known
            .iter()
            .any(|old| !old.revoked && old.fingerprint == key.fingerprint)
    };
    let changed = known.iter().any(|key| key.revoked)
        || scanned
            .iter()
            .any(|key| known.iter().any(|old| old.algorithm == key.algorithm) && !matches(key))
        || (!known.is_empty() && !scanned.iter().any(matches));
    let status = if changed {
        "changed"
    } else if known.is_empty() {
        "new"
    } else {
        "known"
    };
    let mut known_fingerprints: Vec<_> = known.iter().map(|key| key.fingerprint.clone()).collect();
    known_fingerprints.sort();
    known_fingerprints.dedup();
    SshHostKeyInspection {
        status: status.into(),
        keys: scanned
            .iter()
            .map(|key| SshHostKey {
                algorithm: key.algorithm.clone(),
                fingerprint: key.fingerprint.clone(),
            })
            .collect(),
        known_fingerprints,
    }
}

#[tauri::command]
pub async fn inspect_ssh_host_key(
    host: String,
    port: u16,
    user: Option<String>,
) -> Result<SshHostKeyInspection, String> {
    let host = endpoint(&host, port, user.as_deref())?;
    let config = config(&host, port, user.as_deref()).await?;
    let scanned = scan(&host, port).await?;
    Ok(inspection(&scanned, &lookup(&config).await?))
}

fn open_append(path: &Path) -> Result<File, String> {
    let parent = path.parent().ok_or("Invalid known_hosts location")?;
    #[cfg(not(unix))]
    if !parent.exists() {
        let mut builder = std::fs::DirBuilder::new();
        #[cfg(unix)]
        {
            use std::os::unix::fs::DirBuilderExt;
            builder.mode(0o700);
        }
        builder
            .create(parent)
            .map_err(|_| "Create the SSH configuration directory before trusting this host")?;
    }
    #[cfg(not(unix))]
    let file = OpenOptions::new()
        .read(true)
        .append(true)
        .create(true)
        .open(path)
        .map_err(|_| "known_hosts could not be opened safely for append")?;
    #[cfg(unix)]
    {
        return open_append_unix(path, parent);
    }
    #[cfg(not(unix))]
    validate_append_file(file)
}

fn validate_append_file(file: File) -> Result<File, String> {
    let metadata = file
        .metadata()
        .map_err(|_| "Could not inspect known_hosts")?;
    if !metadata.is_file() || metadata.len() > FILE_LIMIT {
        return Err("known_hosts must be a regular file under 4 MiB".into());
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::{MetadataExt, PermissionsExt};
        if metadata.uid() != unsafe { libc::geteuid() } || metadata.nlink() != 1 {
            return Err("known_hosts must be an owned file with no hard links".into());
        }
        file.set_permissions(std::fs::Permissions::from_mode(0o600))
            .map_err(|_| "Could not protect known_hosts")?;
    }
    Ok(file)
}

#[cfg(unix)]
fn secure_directory(path: &Path, create_last: bool) -> Result<File, String> {
    use std::ffi::CString;
    use std::os::fd::{AsRawFd, FromRawFd};
    use std::os::unix::ffi::OsStrExt;
    use std::os::unix::fs::{MetadataExt, OpenOptionsExt};
    let mut directory = OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW | libc::O_DIRECTORY)
        .open("/")
        .map_err(|_| "Could not inspect SSH path root")?;
    let components: Vec<_> = path.components().collect();
    if !path.is_absolute() {
        return Err("known_hosts path must be absolute".into());
    }
    for (index, component) in components.iter().enumerate() {
        let name = match component {
            std::path::Component::RootDir => continue,
            std::path::Component::Normal(value) => *value,
            _ => return Err("known_hosts path must not contain traversal components".into()),
        };
        let name = CString::new(name.as_bytes()).map_err(|_| "Invalid known_hosts directory")?;
        let flags = libc::O_RDONLY | libc::O_NOFOLLOW | libc::O_DIRECTORY | libc::O_CLOEXEC;
        let mut fd = unsafe { libc::openat(directory.as_raw_fd(), name.as_ptr(), flags) };
        if fd < 0
            && create_last
            && index + 1 == components.len()
            && std::io::Error::last_os_error().kind() == std::io::ErrorKind::NotFound
        {
            let result = unsafe { libc::mkdirat(directory.as_raw_fd(), name.as_ptr(), 0o700) };
            if result != 0
                && std::io::Error::last_os_error().kind() != std::io::ErrorKind::AlreadyExists
            {
                return Err("Could not create private SSH directory".into());
            }
            fd = unsafe { libc::openat(directory.as_raw_fd(), name.as_ptr(), flags) };
        }
        if fd < 0 {
            return Err(
                "known_hosts directories must not contain symlinks or inaccessible components"
                    .into(),
            );
        }
        directory = unsafe { File::from_raw_fd(fd) };
        let metadata = directory
            .metadata()
            .map_err(|_| "Could not inspect SSH path directory")?;
        let trusted_owner = metadata.uid() == 0 || metadata.uid() == unsafe { libc::geteuid() };
        // Root-owned sticky shared temp ancestors cannot have our owned
        // descendants renamed by another user. Each descendant is checked.
        let protected_sticky = metadata.uid() == 0 && metadata.mode() & 0o1000 != 0;
        if !trusted_owner || (metadata.mode() & 0o022 != 0 && !protected_sticky) {
            return Err(
                "known_hosts ancestors must not be writable or replaceable by other users".into(),
            );
        }
    }
    let metadata = directory
        .metadata()
        .map_err(|_| "Could not inspect SSH directory")?;
    if metadata.uid() != unsafe { libc::geteuid() } || metadata.mode() & 0o022 != 0 {
        return Err(
            "known_hosts parent must be owned by this user and not writable by others".into(),
        );
    }
    Ok(directory)
}

#[cfg(unix)]
fn open_in_directory(directory: &File, name: &std::ffi::OsStr) -> Result<File, String> {
    use std::ffi::CString;
    use std::os::fd::{AsRawFd, FromRawFd};
    use std::os::unix::ffi::OsStrExt;
    let name = CString::new(name.as_bytes()).map_err(|_| "Invalid known_hosts file name")?;
    let flags = libc::O_RDWR
        | libc::O_APPEND
        | libc::O_CREAT
        | libc::O_NOFOLLOW
        | libc::O_NONBLOCK
        | libc::O_CLOEXEC;
    let fd = unsafe { libc::openat(directory.as_raw_fd(), name.as_ptr(), flags, 0o600) };
    if fd < 0 {
        return Err("known_hosts could not be opened safely for append".into());
    }
    validate_append_file(unsafe { File::from_raw_fd(fd) })
}

#[cfg(unix)]
fn open_append_unix(path: &Path, parent: &Path) -> Result<File, String> {
    let directory = secure_directory(parent, true)?;
    let name = path.file_name().ok_or("Invalid known_hosts file name")?;
    open_in_directory(&directory, name)
}

fn append_line(file: &mut File, path: &Path, key_line: &str) -> Result<(), String> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        let opened = file
            .metadata()
            .map_err(|_| "Could not recheck known_hosts")?;
        let current =
            std::fs::symlink_metadata(path).map_err(|_| "known_hosts changed during trust")?;
        if !current.is_file() || current.dev() != opened.dev() || current.ino() != opened.ino() {
            return Err("known_hosts changed during trust; inspect again".into());
        }
    }
    let len = file
        .metadata()
        .map_err(|_| "Could not recheck known_hosts")?
        .len();
    let newline = if len > 0 {
        file.seek(SeekFrom::End(-1))
            .map_err(|_| "Could not read known_hosts tail")?;
        let mut tail = [0];
        file.read_exact(&mut tail)
            .map_err(|_| "Could not read known_hosts tail")?;
        tail[0] != b'\n'
    } else {
        false
    };
    let line = format!(
        "{}{}\n",
        if newline { "\n" } else { "" },
        key_line.trim_end()
    );
    if len + line.len() as u64 > FILE_LIMIT {
        return Err("known_hosts exceeded its size limit".into());
    }
    file.write_all(line.as_bytes())
        .and_then(|_| file.sync_all())
        .map_err(|_| {
            "Appending host trust did not finish; inspect known_hosts before retrying".to_string()
        })
}

async fn key_line(config: &HostConfig, key: &Key) -> Result<String, String> {
    let line = format!("{} {} {}\n", config.lookup, key.algorithm, key.encoded);
    if !config.hash_new_hosts {
        return Ok(line);
    }
    // Hash only this new record; never rewrite/hash the user's existing file.
    let snapshot = Snapshot::new(line.as_bytes())?;
    let mut command = tool("ssh-keygen");
    command.args(["-H", "-f"]).arg(&snapshot.path);
    let (status, _) = run_tool(command, None, TOOL_TIMEOUT).await?;
    if !status.success() {
        return Err("OpenSSH could not hash the new host record".into());
    }
    let bytes = read_file(&snapshot.path)?.ok_or("Hashed host record disappeared")?;
    let line = String::from_utf8(bytes).map_err(|_| "Invalid hashed host record")?;
    if !line.starts_with("|1|") || line.lines().count() != 1 {
        return Err("Invalid hashed host record".into());
    }
    Ok(line)
}

#[tauri::command]
pub async fn trust_ssh_host_key(
    host: String,
    port: u16,
    expected_fingerprint: String,
    user: Option<String>,
) -> Result<SshHostKeyInspection, String> {
    let host = endpoint(&host, port, user.as_deref())?;
    if !expected_fingerprint
        .strip_prefix("SHA256:")
        .is_some_and(|hash| {
            hash.len() == 43
                && hash
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'+' | b'/'))
        })
    {
        return Err("Expected SHA256 host fingerprint is invalid".into());
    }
    let config = config(&host, port, user.as_deref()).await?;
    let scanned = scan(&host, port).await?;
    trust_scanned(&config, &scanned, &expected_fingerprint).await
}

async fn trust_scanned(
    config: &HostConfig,
    scanned: &[Key],
    expected_fingerprint: &str,
) -> Result<SshHostKeyInspection, String> {
    let _guard = tokio::time::timeout(TOOL_TIMEOUT, TRUST_LOCK.lock())
        .await
        .map_err(|_| "Another host trust operation is in progress")?;
    let selected = scanned
        .iter()
        .find(|key| key.fingerprint == expected_fingerprint)
        .ok_or("SSH host key changed since verification; inspect and verify again")?;
    let before = inspection(scanned, &lookup(config).await?);
    if before.status == "changed" {
        return Err("Existing SSH host key changed or is revoked; no trust was written".into());
    }
    if before.status == "known" {
        if before
            .known_fingerprints
            .iter()
            .any(|key| key == expected_fingerprint)
        {
            return Ok(before);
        }
        return Err(
            "Only entirely new SSH hosts can be trusted here; manage additional keys with OpenSSH"
                .into(),
        );
    }
    let line = key_line(config, selected).await?;
    let target = config
        .user_files
        .first()
        .ok_or("OpenSSH has no writable user known_hosts file configured")?;
    let mut file = open_append(target)?;
    fs2::FileExt::try_lock_exclusive(&file)
        .map_err(|_| "known_hosts is busy; inspect and retry")?;
    // Re-read all sources while holding our append lock. Never add a key if
    // a concurrent trust operation registered a conflicting key meanwhile.
    let current = inspection(scanned, &lookup(config).await?);
    if current.status != "new" {
        if current.status == "known"
            && current
                .known_fingerprints
                .iter()
                .any(|key| key == expected_fingerprint)
        {
            return Ok(current);
        }
        return Err("known_hosts changed during verification; no key was appended".into());
    }
    append_line(&mut file, target, &line)?;
    let after = inspection(scanned, &lookup(config).await?);
    if after.status != "known" {
        return Err(
            "Host trust could not be confirmed; inspect known_hosts before retrying".into(),
        );
    }
    Ok(after)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key(seed: u8) -> Key {
        let mut bytes = Vec::new();
        bytes.extend_from_slice(&11u32.to_be_bytes());
        bytes.extend_from_slice(b"ssh-ed25519");
        bytes.extend_from_slice(&32u32.to_be_bytes());
        bytes.extend_from_slice(&[seed; 32]);
        parse_keys(format!("example.invalid ssh-ed25519 {}\n", STANDARD.encode(bytes)).as_bytes())
            .unwrap()
            .remove(0)
    }

    fn fixture(hash_new_hosts: bool) -> (Snapshot, HostConfig) {
        let snapshot = Snapshot::new(b"").unwrap();
        let config = HostConfig {
            lookup: "[example.invalid]:2222".into(),
            user_files: vec![snapshot.path.clone()],
            global_files: vec![],
            hash_new_hosts,
        };
        (snapshot, config)
    }

    #[test]
    fn endpoint_validation_rejects_options_shell_data_and_zero_port() {
        for host in [
            "",
            "-oProxyCommand=evil",
            "host;id",
            "$(id)",
            "host\nother",
            "host,other",
            "host/a",
        ] {
            assert!(
                endpoint(host, 22, Some("dev")).is_err(),
                "accepted {host:?}"
            );
        }
        assert!(endpoint("safe.invalid", 0, Some("dev")).is_err());
        assert!(endpoint("safe.invalid", 22, Some("-oUser=x")).is_err());
        assert_eq!(
            endpoint(" EXAMPLE.invalid ", 2222, Some("dev")).unwrap(),
            "example.invalid"
        );
        assert!(endpoint("2001:db8::1", 22, Some("dev")).is_ok());
    }

    #[test]
    fn config_uses_effective_known_hosts_and_rejects_ambiguous_routing() {
        let base = "hostname example.invalid\ncanonicalizehostname false\nuserknownhostsfile ~/.ssh/known_hosts ~/.ssh/known_hosts2\nglobalknownhostsfile /etc/ssh/ssh_known_hosts\nhashknownhosts yes\n";
        let config = parse_config(
            base.as_bytes(),
            "example.invalid",
            2222,
            Path::new("/home/test"),
        )
        .unwrap();
        assert_eq!(config.lookup, "[example.invalid]:2222");
        assert_eq!(
            config.user_files[0],
            PathBuf::from("/home/test/.ssh/known_hosts")
        );
        assert!(config.hash_new_hosts);
        let alias = format!("{base}hostkeyalias dedicated-alias\n");
        assert_eq!(
            parse_config(
                alias.as_bytes(),
                "example.invalid",
                2222,
                Path::new("/home/test")
            )
            .unwrap()
            .lookup,
            "dedicated-alias"
        );
        for extra in [
            "proxycommand arbitrary",
            "proxyjump other.invalid",
            "knownhostscommand arbitrary",
            "revokedhostkeys /tmp/revoked",
            "hostkeyalias *.invalid",
            "canonicalizehostname true",
        ] {
            assert!(
                parse_config(
                    format!("{base}{extra}\n").as_bytes(),
                    "example.invalid",
                    2222,
                    Path::new("/home/test")
                )
                .is_err(),
                "accepted {extra}"
            );
        }
        assert!(parse_config(
            base.as_bytes(),
            "other.invalid",
            22,
            Path::new("/home/test")
        )
        .is_err());
    }

    #[test]
    fn classification_preserves_existing_keys_and_revocations() {
        let first = key(1);
        let second = key(2);
        assert_eq!(inspection(&[first.clone()], &[]).status, "new");
        assert_eq!(
            inspection(&[first.clone()], &[first.clone()]).status,
            "known"
        );
        assert_eq!(
            inspection(&[second.clone()], &[first.clone()]).status,
            "changed"
        );
        // Multiple stored rotation keys of the same type are valid if any matches.
        assert_eq!(
            inspection(&[second.clone()], &[first.clone(), second]).status,
            "known"
        );
        let mut revoked = first.clone();
        revoked.revoked = true;
        assert_eq!(inspection(&[first], &[revoked]).status, "changed");
        assert!(parse_keys(b"host ssh-ed25519 $(touch SENTINEL)\n").is_err());
        assert!(parse_keys(b"@cert-authority *.invalid ssh-ed25519 AAAA\n").is_err());
    }

    #[tokio::test]
    async fn openssh_validates_keys_and_matches_hashed_custom_port_hosts() {
        let (_snapshot, config) = fixture(true);
        let selected = key(3);
        validate_keys(&[selected.clone()]).await.unwrap();
        let line = key_line(&config, &selected).await.unwrap();
        assert!(line.starts_with("|1|"));
        assert!(!line.contains("example.invalid"));
        std::fs::write(&config.user_files[0], line).unwrap();
        let known = lookup(&config).await.unwrap();
        assert_eq!(known.len(), 1);
        assert_eq!(known[0].fingerprint, selected.fingerprint);
        let invalid = Key {
            encoded: STANDARD.encode(b"invalid public key"),
            ..selected
        };
        assert!(validate_keys(&[invalid]).await.is_err());
    }

    #[tokio::test]
    async fn openssh_lookup_preserves_wildcards_and_global_host_records() {
        let (_snapshot, mut config) = fixture(false);
        let selected = key(10);
        config.lookup = "example.invalid".into();
        std::fs::write(
            &config.user_files[0],
            format!("*.invalid {} {}\n", selected.algorithm, selected.encoded),
        )
        .unwrap();
        config.global_files = std::mem::take(&mut config.user_files);
        assert_eq!(
            inspection(&[selected.clone()], &lookup(&config).await.unwrap()).status,
            "known"
        );
        // The global file is read-only authority, never a fallback write target.
        assert!(trust_scanned(&config, &[key(11)], &key(11).fingerprint)
            .await
            .is_err());
        assert_eq!(
            lookup(&config).await.unwrap()[0].fingerprint,
            selected.fingerprint
        );
    }

    #[tokio::test]
    async fn snapshot_hash_backup_is_removed_and_existing_tail_is_preserved() {
        let selected = key(12);
        let snapshot =
            Snapshot::new(format!("host {} {}\n", selected.algorithm, selected.encoded).as_bytes())
                .unwrap();
        let directory = snapshot.directory.clone();
        let backup = snapshot.path.with_extension("old");
        let mut command = tool("ssh-keygen");
        command.args(["-H", "-f"]).arg(&snapshot.path);
        assert!(run_tool(command, None, TOOL_TIMEOUT)
            .await
            .unwrap()
            .0
            .success());
        assert!(backup.exists());
        drop(snapshot);
        assert!(!directory.exists());
        let (_snapshot, config) = fixture(false);
        std::fs::write(&config.user_files[0], "# preserve this final comment").unwrap();
        trust_scanned(&config, &[selected.clone()], &selected.fingerprint)
            .await
            .unwrap();
        let text = std::fs::read_to_string(&config.user_files[0]).unwrap();
        assert!(text.starts_with("# preserve this final comment\n"));
        assert_eq!(lookup(&config).await.unwrap().len(), 1);
    }

    #[tokio::test]
    async fn explicit_trust_appends_only_verified_key_and_is_idempotent() {
        let (_snapshot, config) = fixture(true);
        let first = key(4);
        let unverified = key(5);
        let first_different_algorithm = Key {
            algorithm: "ssh-rsa".into(),
            ..unverified.clone()
        };
        let keys = [first.clone(), first_different_algorithm];
        let result = trust_scanned(&config, &keys, &first.fingerprint)
            .await
            .unwrap();
        assert_eq!(result.status, "known");
        let bytes = std::fs::read(&config.user_files[0]).unwrap();
        assert!(!String::from_utf8_lossy(&bytes).contains(&unverified.encoded));
        trust_scanned(&config, &keys, &first.fingerprint)
            .await
            .unwrap();
        assert_eq!(std::fs::read(&config.user_files[0]).unwrap(), bytes);
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(
                std::fs::metadata(&config.user_files[0])
                    .unwrap()
                    .permissions()
                    .mode()
                    & 0o777,
                0o600
            );
        }
    }

    #[tokio::test]
    async fn changed_or_rescanned_fingerprint_cannot_append_or_overwrite() {
        let (_snapshot, config) = fixture(false);
        let old = key(6);
        let changed = key(7);
        let original = format!("{} {} {}\n", config.lookup, old.algorithm, old.encoded);
        std::fs::write(&config.user_files[0], &original).unwrap();
        assert!(
            trust_scanned(&config, &[changed.clone()], &changed.fingerprint)
                .await
                .unwrap_err()
                .contains("changed")
        );
        assert_eq!(
            std::fs::read_to_string(&config.user_files[0]).unwrap(),
            original
        );
        // Key changed between display/approval and rescan: no write even for a new host.
        std::fs::write(&config.user_files[0], "").unwrap();
        assert!(trust_scanned(&config, &[changed], &old.fingerprint)
            .await
            .unwrap_err()
            .contains("since verification"));
        assert!(std::fs::read(&config.user_files[0]).unwrap().is_empty());
    }

    #[tokio::test]
    async fn concurrent_conflicting_trust_never_writes_both_keys() {
        let (_snapshot, config) = fixture(false);
        let one = key(8);
        let two = key(9);
        let one_keys = [one.clone()];
        let two_keys = [two.clone()];
        let (a, b) = tokio::join!(
            trust_scanned(&config, &one_keys, &one.fingerprint),
            trust_scanned(&config, &two_keys, &two.fingerprint)
        );
        assert_ne!(a.is_ok(), b.is_ok());
        assert_eq!(lookup(&config).await.unwrap().len(), 1);
    }

    #[cfg(unix)]
    #[test]
    fn trust_refuses_symlinks_hardlinks_and_replaced_paths() {
        let (snapshot, _) = fixture(false);
        let target = snapshot.directory.join("other");
        std::fs::write(&target, "preserve\n").unwrap();
        let symlink = snapshot.directory.join("link");
        std::os::unix::fs::symlink(&target, &symlink).unwrap();
        assert!(open_append(&symlink).is_err());
        assert!(read_file(&symlink).is_err());
        let hardlink = snapshot.directory.join("hard");
        std::fs::hard_link(&target, &hardlink).unwrap();
        assert!(open_append(&hardlink).is_err());
        let owned = snapshot.directory.join("owned");
        let mut file = open_append(&owned).unwrap();
        std::fs::remove_file(&owned).unwrap();
        std::os::unix::fs::symlink(&target, &owned).unwrap();
        assert!(append_line(&mut file, &owned, "host key encoded").is_err());
        assert_eq!(std::fs::read_to_string(&target).unwrap(), "preserve\n");
        std::fs::remove_file(symlink).unwrap();
        std::fs::remove_file(hardlink).unwrap();
        std::fs::remove_file(owned).unwrap();
        std::fs::remove_file(target).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn parent_replacement_cannot_redirect_append_to_another_owned_file() {
        let (snapshot, _) = fixture(false);
        let parent = snapshot.directory.join("ssh");
        std::fs::create_dir(&parent).unwrap();
        let victim = snapshot.directory.join("victim");
        std::fs::create_dir(&victim).unwrap();
        std::fs::write(victim.join("known_hosts"), "preserve victim\n").unwrap();
        let directory = secure_directory(&parent, false).unwrap();
        let moved = snapshot.directory.join("original-ssh");
        std::fs::rename(&parent, &moved).unwrap();
        std::os::unix::fs::symlink(&victim, &parent).unwrap();
        let mut file = open_in_directory(&directory, std::ffi::OsStr::new("known_hosts")).unwrap();
        assert!(append_line(&mut file, &parent.join("known_hosts"), "host key data").is_err());
        assert_eq!(
            std::fs::read_to_string(victim.join("known_hosts")).unwrap(),
            "preserve victim\n"
        );
        assert!(std::fs::read(moved.join("known_hosts")).unwrap().is_empty());
        assert!(open_append(&parent.join("known_hosts")).is_err());
        drop(file);
        drop(directory);
        std::fs::remove_file(parent).unwrap();
        std::fs::remove_dir_all(moved).unwrap();
        std::fs::remove_dir_all(victim).unwrap();
    }

    #[tokio::test]
    async fn tool_timeout_and_cancellation_kill_owned_child() {
        let (snapshot, _) = fixture(false);
        for cancel in [false, true] {
            let pidfile =
                snapshot
                    .directory
                    .join(if cancel { "cancel.pid" } else { "timeout.pid" });
            let mut command = Command::new("python3");
            command.args(["-c", "import os,sys,time; open(sys.argv[1],'w').write(str(os.getpid())); time.sleep(30)"]).arg(&pidfile);
            let task =
                tokio::spawn(
                    async move { run_tool(command, None, Duration::from_millis(300)).await },
                );
            let pid = tokio::time::timeout(Duration::from_secs(3), async {
                loop {
                    if let Ok(value) = std::fs::read_to_string(&pidfile) {
                        if let Ok(pid) = value.parse::<i32>() {
                            break pid;
                        }
                    }
                    tokio::time::sleep(Duration::from_millis(10)).await;
                }
            })
            .await
            .unwrap();
            if cancel {
                task.abort();
                let _ = task.await;
            } else {
                assert!(task.await.unwrap().unwrap_err().contains("timed out"));
            }
            #[cfg(unix)]
            tokio::time::timeout(Duration::from_secs(3), async {
                while unsafe { libc::kill(pid, 0) } == 0 {
                    tokio::time::sleep(Duration::from_millis(10)).await;
                }
            })
            .await
            .expect("OpenSSH fixture child leaked after timeout/cancel");
            std::fs::remove_file(pidfile).unwrap();
        }
    }

    #[tokio::test]
    async fn tool_output_cap_and_pipe_eof_are_bounded() {
        let mut command = Command::new("python3");
        command.args([
            "-c",
            "import sys; data=sys.stdin.buffer.read(); print(len(data))",
        ]);
        let (status, bytes) = run_tool(command, Some(&vec![b'x'; 200_000]), Duration::from_secs(3))
            .await
            .unwrap();
        assert!(status.success());
        assert_eq!(String::from_utf8(bytes).unwrap().trim(), "200000");
        let mut command = Command::new("python3");
        command.args(["-c", "import sys; sys.stdout.write('x'*200000)"]);
        assert!(run_tool(command, None, Duration::from_secs(3))
            .await
            .unwrap_err()
            .contains("limit"));
    }
}
