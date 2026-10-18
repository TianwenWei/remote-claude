"""Regression tests for the exact Python RPC shipped to Linux SSH hosts."""
import base64
import json
import os
from pathlib import Path
import subprocess
import tempfile
import unittest

HELPER = Path(__file__).with_name("operations.py").read_text()


class RemoteProjectTests(unittest.TestCase):
    def setUp(self):
        self.scratch = tempfile.TemporaryDirectory()
        self.root = Path(self.scratch.name)

    def tearDown(self):
        self.scratch.cleanup()

    def call(self, operation, path=None, **args):
        request = {"root": str(self.root), "path": str(path or self.root), "op": operation, "args": args}
        payload = base64.b64encode(json.dumps(request).encode()).decode()
        result = subprocess.run(["python3", "-c", "REQUEST_B64 = %r\n%s" % (payload, HELPER)], stdout=subprocess.PIPE, stderr=subprocess.PIPE, check=True)
        return json.loads(result.stdout)

    def result(self, operation, path=None, **args):
        response = self.call(operation, path, **args)
        self.assertNotIn("error", response)
        return response["result"]

    def git(self, *args):
        return subprocess.run(["git", "-C", str(self.root), *args], check=True, stdout=subprocess.PIPE, stderr=subprocess.PIPE).stdout

    def init_git(self):
        self.git("init", "-q")

    def test_unicode_and_metacharacters_roundtrip_without_shell_interpretation(self):
        path = self.root / "中文 ' $(touch INJECTION) `id`.txt"
        content = "中文🙂\n' $(id) `whoami`\n"
        self.result("write_file", path, content=content)
        self.assertEqual(self.result("read_file", path)["text"], content)
        self.assertFalse((self.root / "INJECTION").exists())

    def test_root_and_symlink_escape_are_rejected(self):
        self.assertIn("error", self.call("write_file", self.root / "../escape.txt", content="bad"))
        with tempfile.TemporaryDirectory() as outside:
            (self.root / "link").symlink_to(outside, target_is_directory=True)
            self.assertIn("error", self.call("write_file", self.root / "link/escape.txt", content="bad"))
            self.assertFalse((Path(outside) / "escape.txt").exists())
        self.assertIn("error", self.call("rename", to=str(self.root / "moved")))
        self.assertIn("error", self.call("trash"))

    def test_create_duplicate_rename_and_paste_preserve_content(self):
        self.result("create_dir", self.root / "folder")
        original = self.root / "folder/name.txt"
        self.result("create_file", original)
        self.assertIn("error", self.call("create_file", original))
        self.result("write_file", original, content="keep")
        copied = self.result("duplicate", original)
        self.assertEqual(Path(copied["path"]).name, "name copy.txt")
        pasted = self.result("paste", original, to=str(self.root))
        self.assertEqual(Path(pasted["path"]).read_text(), "keep")
        self.result("rename", original, to=str(self.root / "folder/new.txt"))
        self.assertFalse(original.exists())

    def test_binary_image_and_truncated_preview(self):
        (self.root / "binary.bin").write_bytes(b"abc\0def")
        self.assertEqual(self.result("read_file", self.root / "binary.bin")["kind"], "binary")
        (self.root / "image.png").write_bytes(b"fake png")
        self.assertEqual(self.result("read_file", self.root / "image.png")["dataUrl"], "data:image/png;base64,ZmFrZSBwbmc=")
        (self.root / "large.txt").write_text("a" * (1024 * 1024 + 1))
        content = self.result("read_file", self.root / "large.txt")
        self.assertTrue(content["truncated"])
        self.assertEqual(len(content["text"]), 1024 * 1024)

    def test_search_and_index_honor_root_and_gitignore(self):
        self.init_git()
        (self.root / ".gitignore").write_text("ignored/\n")
        (self.root / "ignored").mkdir()
        (self.root / "ignored/a.txt").write_text("hidden output")
        (self.root / "src").mkdir()
        (self.root / "src/main.txt").write_text("中文🙂 Needle\n")
        index = self.result("index", includeIgnored=False)
        self.assertIn({"rel": "src/main.txt", "isDir": False}, index)
        self.assertNotIn({"rel": "ignored/a.txt", "isDir": False}, index)
        self.assertIn({"rel": "ignored/a.txt", "isDir": False}, self.result("index", includeIgnored=True))
        hits = self.result("search", query="needle")
        self.assertEqual(hits, [{"path": str(self.root / "src/main.txt"), "line": 1, "text": "中文🙂 Needle"}])

    def test_git_new_file_diff_stage_and_unstage(self):
        self.init_git()
        (self.root / "new file.txt").write_text("one\ntwo\n")
        status = self.result("git_status")
        self.assertEqual(status["untracked"][0]["path"], "new file.txt")
        self.assertEqual(status["untracked"][0]["additions"], 2)
        patch = self.result("git_diff", file="new file.txt", staged=False)
        self.assertIn("+two", patch)
        self.result("git_stage", files=["new file.txt"])
        self.assertEqual(self.result("git_status")["staged"][0]["additions"], 2)
        self.result("git_unstage", files=["new file.txt"])
        self.assertTrue((self.root / "new file.txt").exists())
        self.assertEqual(len(self.result("git_status")["untracked"]), 1)
        self.assertIn("error", self.call("git_stage", files=["../escape.txt"]))
        self.assertIn("error", self.call("git_stage", files=["/etc/passwd"]))

    def test_git_ancestor_outside_workspace_is_rejected(self):
        self.init_git()
        subdir = self.root / "subdir"
        subdir.mkdir()
        request = {"root": str(subdir), "path": str(subdir), "op": "git_status", "args": {}}
        payload = base64.b64encode(json.dumps(request).encode()).decode()
        result = subprocess.run(["python3", "-c", "REQUEST_B64 = %r\n%s" % (payload, HELPER)], stdout=subprocess.PIPE, check=True)
        self.assertIn("error", json.loads(result.stdout))

    def test_git_tree_tracks_nested_repository_boundaries(self):
        self.init_git()
        (self.root / "new.txt").write_text("new")
        nested = self.root / "nested"
        nested.mkdir()
        subprocess.run(["git", "init", "-q", str(nested)], check=True)
        tree = self.result("git_tree", levels=[{"path": str(self.root), "files": ["new.txt", "nested"], "directories": ["nested"]}])
        colors = tree["fileColors"][str(self.root)]
        self.assertEqual(colors["new.txt"], "untracked")
        self.assertEqual(colors["nested"], "repository")
        self.assertEqual({item["path"] for item in tree["repositories"]}, {str(self.root), str(nested)})

    def test_git_discard_preserves_staged_version(self):
        self.init_git()
        (self.root / "new.txt").write_text("staged\n")
        self.result("git_stage", files=["new.txt"])
        (self.root / "new.txt").write_text("unstaged\n")
        self.result("git_discard", files=["new.txt"])
        self.assertEqual((self.root / "new.txt").read_text(), "staged\n")
        (self.root / "untracked.txt").write_text("delete")
        self.result("git_discard", files=["untracked.txt"])
        self.assertFalse((self.root / "untracked.txt").exists())


if __name__ == "__main__":
    unittest.main()
