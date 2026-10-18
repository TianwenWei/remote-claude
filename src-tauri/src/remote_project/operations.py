# Executed through SSH as a self-contained script. REQUEST_B64 is generated
# by Rust; no user-controlled text is interpolated into shell syntax.
import base64
import datetime
import json
import os
import shutil
import stat
import subprocess
import time
import urllib.parse
import uuid

MAX_READ = 1024 * 1024
MAX_IMAGE = 5 * MAX_READ
MAX_PATCH = 2 * MAX_READ
MAX_ENTRIES = 20000
SKIP_DIRS = {"node_modules", "target", "__pycache__", ".git"}
IMAGE_MIMES = {"png": "image/png", "jpg": "image/jpeg", "jpeg": "image/jpeg", "gif": "image/gif", "webp": "image/webp", "svg": "image/svg+xml", "bmp": "image/bmp", "ico": "image/x-icon"}


def inside(path, root):
    try:
        return os.path.commonpath([path, root]) == root
    except ValueError:
        return False


def checked(path, root, allow_root=True):
    if not isinstance(path, str) or not path.startswith("/") or "\0" in path:
        raise ValueError("remote path must be an absolute Linux path")
    resolved = os.path.realpath(path)
    if not inside(resolved, root) or (not allow_root and resolved == root):
        raise ValueError("path is outside the registered remote workspace or targets its root")
    return resolved


def run_git(path, *args, allowed=(0,)):
    env = dict(os.environ, GIT_TERMINAL_PROMPT="0", GIT_LITERAL_PATHSPECS="1")
    output = subprocess.run(["git", "-C", path, *args], stdout=subprocess.PIPE, stderr=subprocess.PIPE, env=env, timeout=20)
    if output.returncode not in allowed:
        message = output.stderr.decode("utf-8", "replace").strip()
        raise ValueError(message or "git failed with exit code %s" % output.returncode)
    return output.stdout


def repository(path, root):
    try:
        repo = run_git(path, "rev-parse", "--show-toplevel").decode("utf-8", "replace").strip()
    except ValueError:
        raise ValueError("NOT_A_REPO")
    return checked(repo, root)


def git_file(repo, name, root):
    if not isinstance(name, str) or not name or os.path.isabs(name) or "\0" in name or ".." in name.split("/"):
        raise ValueError("Git file paths must be workspace-relative")
    checked(os.path.join(repo, name), root)
    return name


def status_label(code):
    return {"A": "added", "D": "deleted", "R": "renamed", "C": "added", "T": "typechange"}.get(code, "modified")


def numstat(repo, staged):
    result = {}
    args = ["diff", "--numstat", "-z", "--no-renames"]
    if staged:
        args.append("--cached")
    for entry in run_git(repo, *args).split(b"\0"):
        fields = entry.split(b"\t", 2)
        if len(fields) == 3:
            added, deleted, name = fields
            if added.isdigit() and deleted.isdigit():
                result[name.decode("utf-8", "replace")] = (int(added), int(deleted))
    return result


def git_status(repo, root):
    branch = run_git(repo, "symbolic-ref", "--short", "-q", "HEAD", allowed=(0, 1)).decode("utf-8", "replace").strip() or "HEAD"
    result = {"branch": branch, "staged": [], "unstaged": [], "untracked": []}
    entries = run_git(repo, "status", "--porcelain=v1", "-z", "--untracked-files=all").split(b"\0")
    staged_stats, unstaged_stats = numstat(repo, True), numstat(repo, False)
    index = 0
    while index < len(entries):
        raw = entries[index]
        index += 1
        if len(raw) < 4:
            continue
        xy, name = raw[:2].decode("ascii", "replace"), raw[3:].decode("utf-8", "replace")
        if "R" in xy or "C" in xy:
            index += 1  # NUL porcelain emits destination first, then source.
        if xy == "??":
            entry = {"path": name, "status": "added"}
            try:
                full = checked(os.path.join(repo, name), root)
                metadata = os.lstat(full)
                if stat.S_ISREG(metadata.st_mode) and metadata.st_size <= MAX_READ:
                    with open(full, "rb") as stream:
                        data = stream.read(MAX_READ + 1)
                    if b"\0" not in data and len(data) <= MAX_READ:
                        count = data.count(b"\n") + int(bool(data) and not data.endswith(b"\n"))
                        if count <= 100000:
                            entry.update(additions=count, deletions=0)
            except (OSError, ValueError):
                pass
            result["untracked"].append(entry)
            continue
        for bucket, code, stats in [("staged", xy[0], staged_stats), ("unstaged", xy[1], unstaged_stats)]:
            if code not in " ?!":
                entry = {"path": name, "status": status_label(code)}
                if name in stats:
                    entry.update(zip(("additions", "deletions"), stats[name]))
                result[bucket].append(entry)
    counts = run_git(repo, "rev-list", "--left-right", "--count", "HEAD...@{upstream}", allowed=(0, 128)).split()
    if len(counts) == 2 and all(part.isdigit() for part in counts):
        result.update(ahead=int(counts[0]), behind=int(counts[1]))
    return result


def summary(path, root):
    if not os.path.exists(os.path.join(path, ".git")):
        return None
    repo = repository(path, root)
    if repo != path:
        return None
    data = git_status(repo, root)
    return {"path": path, "branch": data["branch"], "changed": len({x["path"] for key in ("staged", "unstaged") for x in data[key]}), "untracked": len(data["untracked"])}


def tree_status(path, root, args):
    result = {"repositories": [], "fileColors": {}}
    seen = set()
    for level in args["levels"]:
        listed = checked(level["path"], root)
        names = set(level["files"])
        colors = {}
        try:
            repo = repository(listed, root)
            data = git_status(repo, root)
            prefix = os.path.relpath(listed, repo)
            prefix = "" if prefix == "." else prefix + "/"
            for bucket in ("untracked", "staged", "unstaged"):
                for entry in data[bucket]:
                    if not entry["path"].startswith(prefix):
                        continue
                    rel = entry["path"][len(prefix):]
                    color = "untracked" if bucket == "untracked" or entry["status"] == "added" else "modified"
                    parts = rel.split("/")
                    for n in range(1, len(parts) + 1):
                        name = "/".join(parts[:n])
                        if name in names and colors.get(name) != "modified":
                            colors[name] = color
        except ValueError as error:
            if str(error) != "NOT_A_REPO":
                raise
        for name in level.get("directories", []):
            if "/" in name or name in (".", ".."):
                raise ValueError("invalid directory name")
            candidate = checked(os.path.join(listed, name), root)
            item = summary(candidate, root)
            if item is not None:
                colors[name] = "repository"
                if candidate not in seen:
                    seen.add(candidate)
                    result["repositories"].append(item)
        item = summary(listed, root)
        if item is not None and listed not in seen:
            seen.add(listed)
            result["repositories"].append(item)
        result["fileColors"][level["path"]] = colors
    return result


def copy_item(source, target, prefer_original, root):
    source = checked(source, root, False)
    target = checked(target, root)
    if not os.path.isdir(target):
        raise ValueError("paste target is not a directory")
    is_dir = os.path.isdir(source)
    if is_dir and inside(target, source):
        raise ValueError("cannot paste a folder into itself or its descendant")
    # Match local copy's refusal to traverse symlinks, including nested ones.
    if os.path.islink(source):
        raise ValueError("cannot copy symlink")
    if is_dir:
        for directory, directories, files in os.walk(source, followlinks=False):
            if any(os.path.islink(os.path.join(directory, name)) for name in directories + files):
                raise ValueError("cannot copy a directory containing symlinks")
    name = os.path.basename(source)
    stem, extension = os.path.splitext(name) if not is_dir else (name, "")
    dest = os.path.join(target, name)
    if not prefer_original or os.path.lexists(dest):
        for counter in range(1000):
            suffix = " copy" if counter == 0 else " copy %s" % counter
            dest = os.path.join(target, stem + suffix + extension)
            if not os.path.lexists(dest):
                break
        else:
            raise ValueError("too many copies exist")
    checked(dest, root, False)
    if is_dir:
        shutil.copytree(source, dest)
    else:
        shutil.copy2(source, dest)
    return {"path": dest, "isDir": is_dir}


def walk_files(path, root):
    deadline = time.monotonic() + 15
    for directory, directories, files in os.walk(path, followlinks=False):
        directories[:] = sorted(name for name in directories if not name.startswith(".") and name not in SKIP_DIRS and not os.path.islink(os.path.join(directory, name)))
        for name in sorted(directories + files):
            if name.startswith("."):
                continue
            candidate = os.path.join(directory, name)
            try:
                checked(candidate, root)
            except ValueError:
                continue
            yield candidate, name in directories
            if time.monotonic() >= deadline:
                return


def file_index(path, root, include_ignored):
    paths = None
    if not include_ignored:
        # git ls-files handles nested .gitignore, info/exclude and global
        # excludes in one process, without an SSH roundtrip per directory.
        try:
            raw = run_git(path, "ls-files", "-z", "--cached", "--others", "--exclude-standard", "--", ".")
            paths = []
            seen = set()
            for item in raw.split(b"\0"):
                rel = item.decode("utf-8", "replace")
                if not rel or any(part.startswith(".") or part in SKIP_DIRS for part in rel.split("/")):
                    continue
                candidate = checked(os.path.join(path, rel), root)
                if not os.path.exists(candidate):
                    continue
                parts = rel.split("/")
                for n in range(1, len(parts) + 1):
                    sub = "/".join(parts[:n])
                    if sub not in seen:
                        paths.append({"rel": sub, "isDir": n < len(parts) or os.path.isdir(candidate)})
                        seen.add(sub)
                    if len(paths) >= MAX_ENTRIES:
                        return paths
        except (ValueError, OSError):
            paths = None
    if paths is not None:
        return paths
    result = []
    for candidate, is_dir in walk_files(path, root):
        result.append({"rel": os.path.relpath(candidate, path), "isDir": is_dir})
        if len(result) >= MAX_ENTRIES:
            break
    return result


def execute(request):
    root_input = request["root"]
    if not isinstance(root_input, str) or not root_input.startswith("/"):
        raise ValueError("registered remote workspace must have an absolute Linux root")
    root = os.path.realpath(root_input)
    if not os.path.isdir(root):
        raise ValueError("remote workspace directory does not exist")
    path = checked(request["path"], root)
    op, args = request["op"], request.get("args", {})
    if op == "list_dir":
        result = []
        with os.scandir(path) as entries:
            for entry in entries:
                try:
                    checked(entry.path, root)
                    meta = entry.stat()
                    result.append({"name": entry.name, "isDir": stat.S_ISDIR(meta.st_mode), "size": meta.st_size, "mtimeMs": int(meta.st_mtime * 1000)})
                except (OSError, ValueError):
                    continue
        return sorted(result, key=lambda item: (not item["isDir"], item["name"]))
    if op == "read_file":
        ext = os.path.splitext(path)[1].lower().lstrip(".")
        limit = MAX_IMAGE if ext in IMAGE_MIMES else MAX_READ
        if ext in IMAGE_MIMES and os.stat(path).st_size > limit:
            raise ValueError("image exceeds 5MB preview limit")
        with open(path, "rb") as stream:
            data = stream.read(limit + 1)
        if ext in IMAGE_MIMES:
            return {"kind": "image", "text": None, "dataUrl": "data:%s;base64,%s" % (IMAGE_MIMES[ext], base64.b64encode(data).decode("ascii")), "truncated": False}
        if b"\0" in data[:8192]:
            return {"kind": "binary", "text": None, "dataUrl": None, "truncated": False}
        return {"kind": "text", "text": data[:limit].decode("utf-8", "replace"), "dataUrl": None, "truncated": len(data) > limit}
    if op in ("write_file", "create_file"):
        checked(path, root, False)
        with open(path, "w" if op == "write_file" else "x", encoding="utf-8") as stream:
            stream.write(args.get("content", ""))
        return None
    if op == "create_dir":
        os.makedirs(path, exist_ok=True)
        return None
    if op == "rename":
        checked(path, root, False)
        target = checked(args["to"], root, False)
        if os.path.lexists(target):
            raise ValueError("rename target already exists")
        os.rename(path, target)
        return None
    if op == "trash":
        checked(path, root, False)
        # Preserve deleted remote files in the VPS user's freedesktop Trash.
        base = os.path.expanduser("~/.local/share/Trash")
        files, info = os.path.join(base, "files"), os.path.join(base, "info")
        os.makedirs(files, mode=0o700, exist_ok=True)
        os.makedirs(info, mode=0o700, exist_ok=True)
        name = os.path.basename(path) + "." + uuid.uuid4().hex
        details = "[Trash Info]\nPath=%s\nDeletionDate=%s\n" % (urllib.parse.quote(path, safe="/"), datetime.datetime.now().strftime("%Y-%m-%dT%H:%M:%S"))
        metadata = os.path.join(info, name + ".trashinfo")
        with open(metadata, "x", encoding="utf-8") as stream:
            stream.write(details)
        try:
            shutil.move(path, os.path.join(files, name))
        except Exception:
            os.unlink(metadata)
            raise
        return None
    if op in ("duplicate", "paste"):
        return copy_item(path, os.path.dirname(path) if op == "duplicate" else args["to"], op == "paste", root)
    if op == "search":
        needle = args["query"].strip().lower()
        if not needle:
            return []
        result = []
        for candidate, is_dir in walk_files(path, root):
            if is_dir:
                continue
            try:
                if os.stat(candidate).st_size > 4 * MAX_READ:
                    continue
                with open(candidate, "r", encoding="utf-8") as stream:
                    for index, line in enumerate(stream, 1):
                        if needle in line.lower():
                            result.append({"path": candidate, "line": index, "text": line.strip()[:200]})
                            if len(result) >= 200:
                                return result
            except (OSError, UnicodeError):
                continue
        return result
    if op == "index":
        return file_index(path, root, args.get("includeIgnored", False))
    if op == "git_tree":
        return tree_status(path, root, args)
    if op == "git_summary":
        return summary(path, root)
    repo = repository(path, root)
    if op == "git_status":
        return git_status(repo, root)
    if op == "git_diff":
        name = git_file(repo, args["file"], root)
        options = ["diff", "--no-ext-diff", "--no-color"]
        if args["staged"]:
            options.append("--cached")
        patch = run_git(repo, *options, "--", name)
        tracked = run_git(repo, "ls-files", "--error-unmatch", "--", name, allowed=(0, 1))
        if not patch and not args["staged"] and not tracked and os.path.isfile(os.path.join(repo, name)):
            patch = run_git(repo, "diff", "--no-index", "--no-ext-diff", "--no-color", "--", "/dev/null", name, allowed=(0, 1))
        text = patch[:MAX_PATCH].decode("utf-8", "replace")
        if len(patch) > MAX_PATCH:
            text += "\n[... diff truncated: file too large to preview ...]\n"
        return text
    if op in ("git_stage", "git_unstage", "git_discard"):
        names = [git_file(repo, name, root) for name in args["files"]]
        if not names:
            return None
        if op == "git_stage":
            run_git(repo, "add", "--all", "--", *names)
        elif op == "git_unstage":
            has_head = bool(run_git(repo, "rev-parse", "--verify", "HEAD", allowed=(0, 128)).strip())
            if has_head:
                run_git(repo, "reset", "-q", "HEAD", "--", *names)
            else:
                run_git(repo, "rm", "--cached", "-q", "--ignore-unmatch", "--", *names)
        else:
            for name in names:
                tracked = bool(run_git(repo, "ls-files", "--error-unmatch", "--", name, allowed=(0, 1)))
                if tracked:
                    run_git(repo, "restore", "--worktree", "--", name)
                else:
                    candidate = checked(os.path.join(repo, name), root, False)
                    if os.path.isfile(candidate):
                        os.unlink(candidate)
        return None
    if op == "git_commit":
        message = args["message"].strip()
        if not message:
            raise ValueError("empty commit message")
        run_git(repo, "commit", "-m", message)
        return run_git(repo, "rev-parse", "HEAD").decode("ascii").strip()
    if op in ("git_push", "git_pull"):
        branch = run_git(repo, "symbolic-ref", "--short", "-q", "HEAD", allowed=(0, 1)).decode("utf-8", "replace").strip()
        if not branch:
            raise ValueError("detached HEAD")
        if op == "git_push":
            run_git(repo, "push", "origin", "refs/heads/%s:refs/heads/%s" % (branch, branch))
        else:
            run_git(repo, "pull", "--ff-only", "origin", branch)
        return None
    if op == "git_branches":
        raw = run_git(repo, "for-each-ref", "--format=%(refname)%00%(symref)", "refs/heads", "refs/remotes")
        result = []
        for line in raw.decode("utf-8", "replace").splitlines():
            ref, symbolic = line.split("\0", 1)
            if symbolic:
                continue
            remote = ref.startswith("refs/remotes/")
            result.append({"name": ref[len("refs/remotes/") if remote else len("refs/heads/"):], "isRemote": remote})
        return sorted(result, key=lambda item: (item["isRemote"], item["name"]))
    if op in ("git_checkout", "git_create_branch"):
        branch = args["branch" if op == "git_checkout" else "name"].strip()
        if not branch or branch.startswith("-"):
            raise ValueError("invalid branch name")
        run_git(repo, "check-ref-format", "--branch", branch)
        if op == "git_create_branch":
            run_git(repo, "switch", "-c", branch)
        else:
            refs = run_git(repo, "for-each-ref", "--format=%(refname)", "refs/heads/" + branch).decode().splitlines()
            if "refs/heads/" + branch in refs:
                run_git(repo, "switch", branch)
            else:
                remotes = run_git(repo, "remote").decode().splitlines()
                remote = next((item for item in sorted(remotes, key=len, reverse=True) if branch.startswith(item + "/")), None)
                if remote:
                    local_name = branch[len(remote) + 1:]
                    local_refs = run_git(repo, "for-each-ref", "--format=%(refname)", "refs/heads/" + local_name).decode().splitlines()
                    if "refs/heads/" + local_name in local_refs:
                        run_git(repo, "switch", local_name)
                    else:
                        run_git(repo, "switch", "--track", "-c", local_name, branch)
                else:
                    run_git(repo, "switch", branch)
        return None
    raise ValueError("unsupported remote project operation: " + op)


def logical_paths(value, canonical_root, registered_root):
    # Registered roots can be symlinks (/srv/current -> /srv/releases/v2).
    # Return paths through the registered spelling so subsequent IPC calls
    # still match the SSH workspace rather than touching a local directory.
    if isinstance(value, list):
        return [logical_paths(item, canonical_root, registered_root) for item in value]
    if isinstance(value, dict):
        result = {}
        for key, item in value.items():
            if key == "path" and isinstance(item, str) and inside(item, canonical_root):
                relative = os.path.relpath(item, canonical_root)
                result[key] = registered_root if relative == "." else os.path.join(registered_root, relative)
            else:
                result[key] = logical_paths(item, canonical_root, registered_root)
        return result
    return value


try:
    request = json.loads(base64.b64decode(REQUEST_B64))
    value = execute(request)
    response = {"result": logical_paths(value, os.path.realpath(request["root"]), request["root"])}
except Exception as error:
    response = {"error": str(error)}
print(json.dumps(response, ensure_ascii=False))
