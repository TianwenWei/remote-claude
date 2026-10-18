//! Only read-only Linux RPCs reuse this private OpenSSH control socket.
//! Mutating RPCs and interactive runs keep dedicated connections.

use std::path::{Path, PathBuf};

use sha2::{Digest, Sha256};

fn socket_name(host: &str, user: &str, port: u16, identity: Option<&str>) -> String {
    let source = serde_json::json!([host.to_ascii_lowercase(), user, port, identity]);
    let digest = Sha256::digest(source.to_string().as_bytes());
    let hash: String = digest[..16]
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect();
    format!("m-{hash}")
}

#[cfg(unix)]
fn private_directory(base: &Path) -> Result<PathBuf, String> {
    use std::os::unix::fs::{DirBuilderExt, MetadataExt, OpenOptionsExt, PermissionsExt};

    let uid = unsafe { libc::geteuid() };
    let dir = base.join(format!("ccgui-ssh-{uid}"));
    match std::fs::DirBuilder::new().mode(0o700).create(&dir) {
        Ok(()) => {}
        Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {}
        Err(error) => {
            return Err(format!(
                "Could not create private SSH socket directory: {error}"
            ))
        }
    }
    let metadata = std::fs::symlink_metadata(&dir).map_err(|error| error.to_string())?;
    if !metadata.file_type().is_dir() || metadata.uid() != uid {
        return Err("SSH socket directory must be a private directory owned by this user".into());
    }
    let directory = std::fs::OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW | libc::O_DIRECTORY)
        .open(&dir)
        .map_err(|error| format!("Could not validate private SSH socket directory: {error}"))?;
    if directory
        .metadata()
        .map_err(|error| error.to_string())?
        .uid()
        != uid
    {
        return Err("SSH socket directory owner changed during validation".into());
    }
    directory
        .set_permissions(std::fs::Permissions::from_mode(0o700))
        .map_err(|error| format!("Could not protect SSH socket directory: {error}"))?;
    Ok(dir)
}

#[cfg(unix)]
fn path_in_directory(
    dir: &Path,
    host: &str,
    user: &str,
    port: u16,
    identity: Option<&str>,
) -> Result<PathBuf, String> {
    use std::os::unix::ffi::OsStrExt;
    use std::os::unix::fs::{FileTypeExt, MetadataExt};

    let path = dir.join(socket_name(host, user, port, identity));
    // OpenSSH appends a random suffix while binding. Leave room inside macOS's
    // 104-byte sun_path, including its NUL, independently of HOME length.
    if path.as_os_str().as_bytes().len() >= 80 {
        return Err("Private SSH socket path is too long".into());
    }
    match std::fs::symlink_metadata(&path) {
        Ok(metadata) => {
            if !metadata.file_type().is_socket() || metadata.uid() != unsafe { libc::geteuid() } {
                return Err("SSH control path must be a socket owned by this user".into());
            }
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => return Err(format!("Could not validate SSH control socket: {error}")),
    }
    Ok(path)
}

pub(super) fn control_path(
    host: &str,
    user: &str,
    port: u16,
    identity: Option<&str>,
) -> Result<Option<PathBuf>, String> {
    #[cfg(unix)]
    {
        // macOS's per-process temp directory is too long for a Unix socket.
        // The user-owned 0700 leaf under this sticky system directory is short.
        #[cfg(target_os = "macos")]
        let base = Path::new("/private/tmp");
        #[cfg(not(target_os = "macos"))]
        let base = Path::new("/tmp");
        let dir = private_directory(base)?;
        path_in_directory(&dir, host, user, port, identity).map(Some)
    }
    #[cfg(not(unix))]
    {
        // Do not assume Unix permission guarantees for a Windows ACL. Keep
        // existing dedicated SSH there until a private socket path is supported.
        let _ = (host, user, port, identity);
        Ok(None)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn socket_hash_partitions_authentication_and_ignores_hostname_case() {
        let original = socket_name("VPS.example", "cc", 22, Some("/key/a"));
        assert_eq!(
            original,
            socket_name("vps.example", "cc", 22, Some("/key/a"))
        );
        for changed in [
            socket_name("other.example", "cc", 22, Some("/key/a")),
            socket_name("vps.example", "other", 22, Some("/key/a")),
            socket_name("vps.example", "cc", 2222, Some("/key/a")),
            socket_name("vps.example", "cc", 22, Some("/key/b")),
            socket_name("vps.example", "cc", 22, None),
        ] {
            assert_ne!(original, changed);
        }
        assert_eq!(original.len(), 34);
        assert!(!original.contains("VPS"));
        assert!(!original.contains("key"));
    }

    #[cfg(unix)]
    struct Scratch(PathBuf);

    #[cfg(unix)]
    impl Scratch {
        fn new() -> Self {
            let root = PathBuf::from("/private/tmp").join(format!(
                "mux-{}",
                &uuid::Uuid::new_v4().simple().to_string()[..8]
            ));
            #[cfg(not(target_os = "macos"))]
            let root = PathBuf::from("/tmp").join(root.file_name().unwrap());
            std::fs::create_dir(&root).unwrap();
            Self(root)
        }
    }

    #[cfg(unix)]
    impl Drop for Scratch {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    #[cfg(unix)]
    #[test]
    fn socket_directory_is_private_and_symlink_or_file_replacements_are_refused() {
        use std::os::unix::fs::{symlink, PermissionsExt};
        let scratch = Scratch::new();
        let dir = private_directory(&scratch.0).unwrap();
        assert_eq!(
            std::fs::metadata(&dir).unwrap().permissions().mode() & 0o777,
            0o700
        );
        std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o755)).unwrap();
        assert_eq!(private_directory(&scratch.0).unwrap(), dir);
        assert_eq!(
            std::fs::metadata(&dir).unwrap().permissions().mode() & 0o777,
            0o700
        );
        std::fs::remove_dir(&dir).unwrap();
        let outside = scratch.0.join("outside");
        std::fs::create_dir(&outside).unwrap();
        std::fs::set_permissions(&outside, std::fs::Permissions::from_mode(0o755)).unwrap();
        symlink(&outside, &dir).unwrap();
        assert!(private_directory(&scratch.0).is_err());
        assert_eq!(
            std::fs::metadata(&outside).unwrap().permissions().mode() & 0o777,
            0o755
        );
        std::fs::remove_file(&dir).unwrap();
        std::fs::write(&dir, "unrelated file").unwrap();
        assert!(private_directory(&scratch.0).is_err());
    }

    #[cfg(unix)]
    #[test]
    fn socket_path_accepts_only_owned_socket_and_refuses_oversized_paths() {
        use std::os::unix::{fs::symlink, net::UnixListener};
        let scratch = Scratch::new();
        let dir = private_directory(&scratch.0).unwrap();
        let path = path_in_directory(&dir, "host", "cc", 22, None).unwrap();
        let socket = UnixListener::bind(&path).unwrap();
        assert_eq!(
            path_in_directory(&dir, "host", "cc", 22, None).unwrap(),
            path
        );
        drop(socket);
        std::fs::remove_file(&path).unwrap();
        std::fs::write(&path, "not a socket").unwrap();
        assert!(path_in_directory(&dir, "host", "cc", 22, None).is_err());
        std::fs::remove_file(&path).unwrap();
        symlink(scratch.0.join("target"), &path).unwrap();
        assert!(path_in_directory(&dir, "host", "cc", 22, None).is_err());
        assert!(path_in_directory(
            Path::new(&format!("/{}", "x".repeat(100))),
            "host",
            "cc",
            22,
            None
        )
        .is_err());
    }
}
