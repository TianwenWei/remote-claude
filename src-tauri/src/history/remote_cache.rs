//! Full transcript snapshots and their verified remote provenance. A local
//! read never contacts SSH, and an unchanged remote stat avoids retransferring
//! the payload. The manifest hashes the bytes so two-file publication races
//! cannot bless the wrong snapshot as fresh.
use base64::Engine as _;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};

pub(super) const MAX_BYTES: u64 = 64 * 1024 * 1024;

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub(super) struct RemoteStat {
    pub size: u64,
    pub modified: String,
    #[serde(default)]
    pub changed: Option<String>,
    #[serde(default)]
    pub inode: Option<u64>,
}

#[derive(Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
struct Manifest {
    version: u8,
    source: String,
    stat: RemoteStat,
    content_hash: String,
    cached_at: u64,
}

pub(super) struct Snapshot {
    pub bytes: Vec<u8>,
    pub cached_at: u64,
    manifest: Option<Manifest>,
}

impl Snapshot {
    pub fn matches(&self, stat: &RemoteStat) -> bool {
        self.manifest.as_ref().is_some_and(|m| &m.stat == stat)
    }
    pub fn is_legacy(&self) -> bool {
        self.manifest.is_none()
    }
    pub fn hash(&self) -> String {
        content_hash(&self.bytes)
    }
}

#[derive(Clone)]
pub(super) struct TranscriptCache {
    pub path: PathBuf,
    manifest_path: PathBuf,
}

impl TranscriptCache {
    pub fn new(dir: &Path, workspace: &str, engine: &str, sid: &str, remote: &str) -> Self {
        // Keep existing filenames so verified legacy snapshots can migrate
        // without downloading their bytes again.
        let digest = Sha256::digest(format!("{workspace}|{engine}|{sid}|{remote}"));
        let name: String = digest[..16].iter().map(|b| format!("{b:02x}")).collect();
        Self {
            path: dir.join(format!("{name}.jsonl")),
            manifest_path: dir.join(format!("{name}.meta.json")),
        }
    }

    pub fn read(&self, source: &str, allow_legacy: bool) -> Result<Option<Snapshot>, String> {
        if !cache_directory(self.path.parent().ok_or("历史缓存目录无效")?, false)? {
            return Ok(None);
        }
        let Some(file) = regular_file(&self.path)? else {
            return Ok(None);
        };
        let metadata = file.metadata().map_err(|e| e.to_string())?;
        if metadata.len() > MAX_BYTES {
            return Ok(None);
        }
        let mut bytes = Vec::new();
        file.take(MAX_BYTES + 1)
            .read_to_end(&mut bytes)
            .map_err(|e| e.to_string())?;
        if bytes.len() as u64 > MAX_BYTES {
            return Ok(None);
        }
        let manifest = match regular_file(&self.manifest_path)? {
            Some(file) => {
                let mut raw = Vec::new();
                file.take(16 * 1024 + 1)
                    .read_to_end(&mut raw)
                    .map_err(|e| e.to_string())?;
                if raw.len() > 16 * 1024 {
                    return Ok(None);
                }
                let m: Manifest = match serde_json::from_slice(&raw) {
                    Ok(m) => m,
                    Err(_) => return Ok(None),
                };
                if m.version != 1
                    || m.source != source
                    || m.stat.size != bytes.len() as u64
                    || m.content_hash != content_hash(&bytes)
                {
                    return Ok(None);
                }
                Some(m)
            }
            None if allow_legacy => None,
            None => return Ok(None),
        };
        let cached_at = manifest.as_ref().map(|m| m.cached_at).unwrap_or_else(|| {
            metadata
                .modified()
                .ok()
                .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                .map(|d| d.as_millis() as u64)
                .unwrap_or(0)
        });
        Ok(Some(Snapshot {
            bytes,
            cached_at,
            manifest,
        }))
    }

    /// Returns whether the transcript bytes changed. Every temporary file is
    /// private and unique; a failed publication keeps the previous cache.
    pub fn publish(&self, bytes: &[u8], source: &str, stat: RemoteStat) -> Result<bool, String> {
        if bytes.len() as u64 > MAX_BYTES || stat.size != bytes.len() as u64 {
            return Err("历史快照字节数与远程元数据不一致".into());
        }
        let dir = self.path.parent().ok_or("历史缓存目录无效")?;
        cache_directory(dir, true)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
            std::fs::OpenOptions::new()
                .read(true)
                .custom_flags(libc::O_NOFOLLOW | libc::O_DIRECTORY)
                .open(dir)
                .map_err(|e| e.to_string())?
                .set_permissions(std::fs::Permissions::from_mode(0o700))
                .map_err(|e| e.to_string())?;
        }
        let changed = match regular_file(&self.path)? {
            Some(file) => {
                let mut old = Vec::new();
                file.take(MAX_BYTES + 1)
                    .read_to_end(&mut old)
                    .map_err(|e| e.to_string())?;
                old != bytes
            }
            None => true,
        };
        if changed {
            atomic_write(&self.path, bytes)?;
        } else {
            private_file(&self.path)?;
        }
        let manifest = Manifest {
            version: 1,
            source: source.to_string(),
            stat,
            content_hash: content_hash(bytes),
            cached_at: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map_err(|e| e.to_string())?
                .as_millis() as u64,
        };
        atomic_write(
            &self.manifest_path,
            &serde_json::to_vec(&manifest).map_err(|e| e.to_string())?,
        )?;
        Ok(changed)
    }

    pub fn purge(&self) -> Result<(), String> {
        if !cache_directory(self.path.parent().ok_or("历史缓存目录无效")?, false)? {
            return Ok(());
        }
        for path in [&self.path, &self.manifest_path] {
            match std::fs::remove_file(path) {
                Ok(()) => (),
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => (),
                Err(e) => return Err(format!("删除本地历史缓存失败: {e}")),
            }
        }
        Ok(())
    }
}

fn cache_directory(path: &Path, create: bool) -> Result<bool, String> {
    match std::fs::symlink_metadata(path) {
        Ok(meta) if meta.file_type().is_symlink() || !meta.is_dir() => {
            Err("历史缓存目录必须为真实目录".into())
        }
        Ok(_) => Ok(true),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound && create => {
            std::fs::create_dir_all(path).map_err(|e| e.to_string())?;
            cache_directory(path, false)
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(e) => Err(e.to_string()),
    }
}

fn regular_file(path: &Path) -> Result<Option<std::fs::File>, String> {
    match std::fs::symlink_metadata(path) {
        Ok(meta) if meta.file_type().is_symlink() || !meta.is_file() => {
            return Err("历史缓存必须为普通文件".into())
        }
        Ok(_) => (),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(e.to_string()),
    }
    let mut options = std::fs::OpenOptions::new();
    options.read(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.custom_flags(libc::O_NOFOLLOW);
    }
    options.open(path).map(Some).map_err(|e| e.to_string())
}

fn private_file(path: &Path) -> Result<(), String> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        regular_file(path)?
            .ok_or("历史缓存缺失")?
            .set_permissions(std::fs::Permissions::from_mode(0o600))
            .map_err(|e| e.to_string())?;
    }
    Ok(())
}

fn atomic_write(path: &Path, bytes: &[u8]) -> Result<(), String> {
    let tmp = path.with_extension(format!("{}.tmp", uuid::Uuid::new_v4().simple()));
    let result = (|| {
        let mut options = std::fs::OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        let mut file = options.open(&tmp).map_err(|e| e.to_string())?;
        file.write_all(bytes).map_err(|e| e.to_string())?;
        file.sync_all().map_err(|e| e.to_string())?;
        std::fs::rename(&tmp, path).map_err(|e| format!("缓存落位失败: {e}"))
    })();
    if result.is_err() {
        let _ = std::fs::remove_file(tmp);
    }
    result
}

pub(super) fn content_hash(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

#[derive(Deserialize)]
pub(super) struct StatResponse {
    pub stat: RemoteStat,
    pub sha256: Option<String>,
}

#[derive(Deserialize)]
struct TransferResponse {
    stat: RemoteStat,
    sha256: String,
    encoding: String,
    data: String,
}

pub(super) fn decode_transfer(raw: &str) -> Result<(Vec<u8>, RemoteStat), String> {
    decode_transfer_limited(raw, MAX_BYTES)
}

fn decode_transfer_limited(raw: &str, maximum: u64) -> Result<(Vec<u8>, RemoteStat), String> {
    let response: TransferResponse =
        serde_json::from_str(raw).map_err(|e| format!("远程历史响应无效: {e}"))?;
    if response.stat.size > maximum || response.data.len() as u64 > (maximum + 2) / 3 * 4 + 4 {
        return Err("远程会话文件超过64MB上限".into());
    }
    let encoded = base64::engine::general_purpose::STANDARD
        .decode(&response.data)
        .map_err(|e| format!("远程转录本解码失败: {e}"))?;
    let bytes = match response.encoding.as_str() {
        "base64" => encoded,
        "gzip+base64" => {
            let mut bytes = Vec::new();
            flate2::read::GzDecoder::new(encoded.as_slice())
                .take(maximum + 1)
                .read_to_end(&mut bytes)
                .map_err(|e| format!("远程历史解压失败: {e}"))?;
            bytes
        }
        _ => return Err("远程历史编码无效".into()),
    };
    if bytes.len() as u64 > maximum
        || bytes.len() as u64 != response.stat.size
        || content_hash(&bytes) != response.sha256
    {
        return Err("远程历史大小或校验不一致".into());
    }
    Ok((bytes, response.stat))
}

pub(super) const STAT_PY: &str = r#"
import hashlib,json,os,sys
path,maximum,need_hash=sys.argv[1],int(sys.argv[2]),sys.argv[3]=='1'
def signature(s): return {'size':s.st_size,'modified':str(s.st_mtime_ns),'changed':str(s.st_ctime_ns),'inode':s.st_ino}
with open(path,'rb') as f:
    before=os.fstat(f.fileno())
    if before.st_size>maximum: raise RuntimeError('Remote transcript exceeds 64MB limit')
    digest=None
    if need_hash:
        data=f.read(maximum+1)
        after=os.fstat(f.fileno())
        if len(data)>maximum or signature(before)!=signature(after) or len(data)!=after.st_size:
            raise RuntimeError('Remote transcript changed while verifying cache; retry')
        digest=hashlib.sha256(data).hexdigest()
print(json.dumps({'stat':signature(before),'sha256':digest}))
"#;

pub(super) const TRANSFER_PY: &str = r#"
import base64,gzip,hashlib,json,os,sys
path,maximum=sys.argv[1],int(sys.argv[2])
def signature(s): return {'size':s.st_size,'modified':str(s.st_mtime_ns),'changed':str(s.st_ctime_ns),'inode':s.st_ino}
with open(path,'rb') as f:
    before=os.fstat(f.fileno())
    if before.st_size>maximum: raise RuntimeError('Remote transcript exceeds 64MB limit')
    data=f.read(maximum+1)
    after=os.fstat(f.fileno())
if len(data)>maximum: raise RuntimeError('Remote transcript exceeds 64MB limit')
if signature(before)!=signature(after) or len(data)!=after.st_size:
    raise RuntimeError('Remote transcript changed while reading; retry')
compressed=gzip.compress(data,compresslevel=6,mtime=0)
payload,encoding=(compressed,'gzip+base64') if len(compressed)<len(data) else (data,'base64')
print(json.dumps({'stat':signature(after),'sha256':hashlib.sha256(data).hexdigest(),'encoding':encoding,'data':base64.b64encode(payload).decode('ascii')}))
"#;

#[cfg(test)]
mod tests {
    use super::*;

    struct Scratch(PathBuf);
    impl Scratch {
        fn new() -> Self {
            let dir =
                std::env::temp_dir().join(format!("ccgui-remote-cache-{}", uuid::Uuid::new_v4()));
            std::fs::create_dir_all(&dir).unwrap();
            Self(dir)
        }
        fn cache(&self) -> TranscriptCache {
            TranscriptCache::new(
                &self.0.join("cache"),
                "/workspace",
                "claude",
                "sid",
                "/home/a/projects/-workspace/sid.jsonl",
            )
        }
    }
    impl Drop for Scratch {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }
    fn stat(size: u64, modified: &str) -> RemoteStat {
        RemoteStat {
            size,
            modified: modified.into(),
            changed: Some("ctime".into()),
            inode: Some(42),
        }
    }

    #[test]
    fn cache_stat_reuses_unchanged_snapshot_and_invalidates_growth_and_same_size_rewrite() {
        let scratch = Scratch::new();
        let cache = scratch.cache();
        let a = stat(3, "1000000001");
        cache.publish(b"one", "source-a", a.clone()).unwrap();
        let snapshot = cache.read("source-a", false).unwrap().unwrap();
        assert!(snapshot.matches(&a));
        assert!(!snapshot.matches(&stat(4, "1000000001")));
        assert!(!snapshot.matches(&stat(3, "1000000002")));
        let mut replaced = a.clone();
        replaced.inode = Some(43);
        assert!(!snapshot.matches(&replaced));
        assert!(cache.read("source-b", false).unwrap().is_none());
        assert!(snapshot.cached_at > 0);
    }

    #[test]
    fn legacy_snapshot_requires_remote_verification_and_corruption_is_a_cache_miss() {
        let scratch = Scratch::new();
        let cache = scratch.cache();
        std::fs::create_dir_all(cache.path.parent().unwrap()).unwrap();
        std::fs::write(&cache.path, b"old").unwrap();
        assert!(cache.read("source", false).unwrap().is_none());
        let legacy = cache.read("source", true).unwrap().unwrap();
        assert!(legacy.is_legacy());
        cache
            .publish(&legacy.bytes, "source", stat(3, "1"))
            .unwrap();
        assert!(!cache.read("source", false).unwrap().unwrap().is_legacy());
        std::fs::write(&cache.path, b"bad").unwrap();
        assert!(cache.read("source", false).unwrap().is_none());
        cache.publish(b"new", "source", stat(3, "2")).unwrap();
        assert_eq!(cache.read("source", false).unwrap().unwrap().bytes, b"new");
        let file = std::fs::File::options()
            .write(true)
            .open(&cache.path)
            .unwrap();
        file.set_len(MAX_BYTES + 1).unwrap();
        assert!(cache.read("source", true).unwrap().is_none());
        cache.publish(b"ok", "source", stat(2, "3")).unwrap();
        cache.purge().unwrap();
        assert!(cache.read("source", false).unwrap().is_none());
        assert!(!cache.manifest_path.exists());
    }

    #[test]
    fn unique_atomic_temps_support_concurrent_publish_without_missing_rename() {
        let scratch = Scratch::new();
        let cache = scratch.cache();
        let workers: Vec<_> = (0..8)
            .map(|n| {
                let cache = cache.clone();
                std::thread::spawn(move || {
                    cache.publish(b"same", "source", stat(4, &n.to_string()))
                })
            })
            .collect();
        for worker in workers {
            worker.join().unwrap().unwrap();
        }
        assert_eq!(cache.read("source", false).unwrap().unwrap().bytes, b"same");
        assert_eq!(
            std::fs::read_dir(cache.path.parent().unwrap())
                .unwrap()
                .count(),
            2
        );
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(
                std::fs::metadata(cache.path.parent().unwrap())
                    .unwrap()
                    .permissions()
                    .mode()
                    & 0o777,
                0o700
            );
            assert_eq!(
                std::fs::metadata(&cache.path).unwrap().permissions().mode() & 0o777,
                0o600
            );
            assert_eq!(
                std::fs::metadata(&cache.manifest_path)
                    .unwrap()
                    .permissions()
                    .mode()
                    & 0o777,
                0o600
            );
        }
    }

    #[cfg(unix)]
    #[test]
    fn cache_rejects_symlink_files_and_directories_without_chmodding_target() {
        use std::os::unix::fs::{symlink, PermissionsExt};
        let scratch = Scratch::new();
        let cache = scratch.cache();
        let outside = scratch.0.join("unrelated");
        std::fs::write(&outside, b"same").unwrap();
        std::fs::set_permissions(&outside, std::fs::Permissions::from_mode(0o644)).unwrap();
        std::fs::create_dir_all(cache.path.parent().unwrap()).unwrap();
        symlink(&outside, &cache.path).unwrap();
        assert!(cache.read("source", true).is_err());
        assert!(cache.publish(b"same", "source", stat(4, "1")).is_err());
        assert_eq!(
            std::fs::metadata(&outside).unwrap().permissions().mode() & 0o777,
            0o644
        );
        std::fs::remove_file(&cache.path).unwrap();
        std::fs::remove_dir(cache.path.parent().unwrap()).unwrap();
        symlink(&scratch.0, cache.path.parent().unwrap()).unwrap();
        assert!(cache.publish(b"same", "source", stat(4, "1")).is_err());
    }

    fn python(script: &str, path: &Path, maximum: u64, hash: bool) -> std::process::Output {
        std::process::Command::new("python3")
            .arg("-c")
            .arg(script)
            .arg(path)
            .arg(maximum.to_string())
            .arg(if hash { "1" } else { "0" })
            .output()
            .unwrap()
    }

    #[test]
    fn python_transfer_roundtrips_unicode_and_uses_compression_only_when_smaller() {
        let scratch = Scratch::new();
        let path = scratch.0.join("session.jsonl");
        let bytes = "中文🙂 repeated text\n".repeat(400).into_bytes();
        std::fs::write(&path, &bytes).unwrap();
        let output = python(TRANSFER_PY, &path, MAX_BYTES, false);
        assert!(output.status.success());
        let response: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
        assert_eq!(response["encoding"], "gzip+base64");
        let decoded = decode_transfer(std::str::from_utf8(&output.stdout).unwrap()).unwrap();
        assert_eq!(decoded.0, bytes);
        let metadata = python(STAT_PY, &path, MAX_BYTES, true);
        let metadata: StatResponse = serde_json::from_slice(&metadata.stdout).unwrap();
        assert_eq!(metadata.stat, decoded.1);
        assert_eq!(metadata.sha256.unwrap(), content_hash(&bytes));
        std::fs::write(&path, b"x").unwrap();
        let output = python(TRANSFER_PY, &path, MAX_BYTES, false);
        let response: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
        assert_eq!(response["encoding"], "base64");
        assert!(!python(TRANSFER_PY, &path, 0, false).status.success());
    }

    #[test]
    fn compressed_decode_caps_expansion_and_verifies_content_hash() {
        let bytes = vec![b'a'; 100_000];
        let mut gzip = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
        gzip.write_all(&bytes).unwrap();
        let data = gzip.finish().unwrap();
        let mut response = serde_json::json!({"stat":stat(100, "1"),"sha256":content_hash(&bytes),
            "encoding":"gzip+base64","data":base64::engine::general_purpose::STANDARD.encode(data)});
        assert!(decode_transfer_limited(&response.to_string(), 1024).is_err());
        response["stat"] = serde_json::to_value(stat(bytes.len() as u64, "1")).unwrap();
        assert_eq!(decode_transfer(&response.to_string()).unwrap().0, bytes);
        response["sha256"] = "invalid".into();
        assert!(decode_transfer(&response.to_string()).is_err());
    }
}
