#!/usr/bin/env python3
"""Build and package Remote Claude without copying local user data."""
import argparse
import datetime
import hashlib
import json
import os
from pathlib import Path
import plistlib
import shutil
import subprocess
import sys
import tempfile


REPO = Path(__file__).resolve().parent.parent
TARGETS = ("aarch64-apple-darwin", "x86_64-apple-darwin", "universal-apple-darwin")


def run(args, **kwargs):
    return subprocess.run([str(arg) for arg in args], check=True, **kwargs)


def sha256(path):
    digest = hashlib.sha256()
    with path.open("rb") as file:
        for block in iter(lambda: file.read(1024 * 1024), b""):
            digest.update(block)
    return digest.hexdigest()


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--target", choices=TARGETS, default="aarch64-apple-darwin")
    parser.add_argument("--output", type=Path, default=REPO / "artifacts/remote-claude-trial")
    signing = parser.add_mutually_exclusive_group(required=True)
    signing.add_argument("--ad-hoc", action="store_true", help="Private trial only; not notarized")
    signing.add_argument("--signing-identity", help="Your Developer ID Application identity")
    parser.add_argument("--notary-profile", help="Existing notarytool keychain profile")
    args = parser.parse_args()
    if sys.platform != "darwin":
        parser.error("macOS is required")
    if args.notary_profile and args.ad_hoc:
        parser.error("Notarization requires a Developer ID Application identity")
    if args.signing_identity and not args.signing_identity.startswith("Developer ID Application:"):
        parser.error("Use a Developer ID Application certificate for distribution")

    output = args.output.expanduser().resolve()
    output.mkdir(parents=True, exist_ok=True)
    config = json.loads((REPO / "src-tauri/tauri.conf.json").read_text())
    remote_config = json.loads((REPO / "src-tauri/tauri.remote.conf.json").read_text())
    version = remote_config.get("version", config["version"])
    arch = {TARGETS[0]: "arm64", TARGETS[1]: "intel", TARGETS[2]: "universal"}[args.target]
    stem = f"Remote-Claude-{version}-trial-{arch}"
    dmg = output / f"{stem}.dmg"
    archive = output / f"{stem}.zip"
    manifest_path = output / f"{stem}.json"
    if any(path.exists() for path in (dmg, archive, manifest_path)):
        raise RuntimeError("Package already exists; choose a different --output directory")
    identity = "-" if args.ad_hoc else args.signing_identity
    overlay = json.dumps({"bundle": {"macOS": {"signingIdentity": identity}}})
    # Keep the remote-vps feature and identity config paired in the existing script.
    run([REPO / "scripts/build-remote-macos.sh", "--target", args.target, "--config", overlay], cwd=REPO)
    app = REPO / "src-tauri/target" / args.target / "release/bundle/macos/Remote Claude.app"
    info = plistlib.loads((app / "Contents/Info.plist").read_bytes())
    if info.get("CFBundleIdentifier") != "local.ccgui.remote-vps" or info.get("CFBundleName") != "Remote Claude":
        raise RuntimeError("The build did not produce the isolated Remote Claude app")
    run(["codesign", "--verify", "--deep", "--strict", app])
    if info["CFBundleShortVersionString"] != version:
        raise RuntimeError("Built app version does not match the package configuration")

    with tempfile.TemporaryDirectory(prefix="remote-claude-package-") as temporary:
        stage = Path(temporary) / "Remote Claude"
        stage.mkdir()
        run(["ditto", app, stage / app.name])
        (stage / "Applications").symlink_to("/Applications")
        shutil.copyfile(REPO / "docs/remote-claude-trial.zh-CN.md", stage / "开始使用.md")
        shutil.copyfile(REPO / "UPSTREAM-NOTICE.md", stage / "UPSTREAM-NOTICE.md")
        status = (
            "个人试用包：ad-hoc 签名，未经 Apple 公证。安装时 macOS 可能阻止打开。\n"
            "请阅读开始使用.md；本包不要求关闭 Gatekeeper 或移除系统安全检查。\n"
            if args.ad_hoc else
            "使用 Developer ID 签名。公证状态见随附 JSON 清单。\n"
        )
        (stage / "签名状态.txt").write_text(status, encoding="utf-8")
        run(["hdiutil", "create", "-volname", "Remote Claude Trial", "-srcfolder", stage,
             "-format", "UDZO", "-ov", dmg])

    if args.signing_identity:
        run(["codesign", "--force", "--sign", identity, "--timestamp", dmg])
    if args.notary_profile:
        # Notary credentials remain in the user's Keychain, never in this repository.
        run(["xcrun", "notarytool", "submit", dmg, "--keychain-profile", args.notary_profile, "--wait"])
        run(["xcrun", "stapler", "staple", dmg])
        run(["xcrun", "stapler", "validate", dmg])
        run(["xcrun", "stapler", "staple", app])
        run(["xcrun", "stapler", "validate", app])
    run(["ditto", "-c", "-k", "--keepParent", app, archive])
    revision_result = subprocess.run(["git", "rev-parse", "HEAD"], cwd=REPO, text=True, capture_output=True)
    revision = revision_result.stdout.strip() if revision_result.returncode == 0 else "unknown (source archive)"
    clean = (subprocess.run(["git", "diff", "--quiet", "HEAD"], cwd=REPO, capture_output=True).returncode == 0
             if revision_result.returncode == 0 else None)
    manifest = {
        "name": "Remote Claude", "version": version, "channel": "friends-trial",
        "architecture": arch, "target": args.target, "sourceRevision": revision,
        "trackedSourcesClean": clean,
        "createdAt": datetime.datetime.now(datetime.timezone.utc).isoformat(),
        "bundleIdentifier": info["CFBundleIdentifier"], "dataDirectory": "~/.ccgui-remote-vps",
        "signature": "ad-hoc" if args.ad_hoc else "Developer ID Application",
        "notarized": bool(args.notary_profile),
        "files": {path.name: {"sha256": sha256(path), "bytes": path.stat().st_size} for path in (dmg, archive)},
    }
    manifest_path.write_text(json.dumps(manifest, ensure_ascii=False, indent=2) + "\n", encoding="utf-8")
    checksums = output / f"{stem}.sha256"
    checksums.write_text("".join(f"{sha256(path)}  {path.name}\n" for path in (dmg, archive, manifest_path)), encoding="utf-8")
    print(f"Packages: {output}\nManifest: {manifest_path}\nNotarized: {manifest['notarized']}")


if __name__ == "__main__":
    main()
