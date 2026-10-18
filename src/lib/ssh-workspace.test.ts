import { describe, expect, it } from "vitest";
import type { Workspace } from "./ipc";
import { isSshWorkspacePath, sshWorkspaceMetaOf } from "./ssh-workspace";

const workspace: Workspace = { id: "ssh", name: "project", path: "/srv/project", lastOpenedAt: null,
  sortOrder: null, groupId: null, meta: { ssh: { host: "vps", user: "cc", port: 22, identityFile: null,
    workspace: "/srv/project", enginePaths: { claude: "/usr/bin/claude", malformed: 42 } } } };

describe("SSH workspace metadata", () => {
  it("validates native connection metadata and only accepts discovered binary paths", () => {
    expect(sshWorkspaceMetaOf(workspace)?.enginePaths).toEqual({ claude: "/usr/bin/claude" });
    expect(sshWorkspaceMetaOf({ ...workspace, meta: { ssh: { ...(workspace.meta!.ssh as object), port: 65536 } } })).toBeNull();
    expect(sshWorkspaceMetaOf({ ...workspace, meta: { ssh: "not metadata" } })).toBeNull();
    expect(sshWorkspaceMetaOf({ ...workspace, meta: undefined })).toBeNull();
  });

  it("matches the remote root and its children without capturing an unrelated local prefix", () => {
    expect(isSshWorkspacePath([workspace], "/srv/project")).toBe(true);
    expect(isSshWorkspacePath([workspace], "/srv/project/src/main.ts")).toBe(true);
    expect(isSshWorkspacePath([workspace], "/srv/project-copy/main.ts")).toBe(false);
    expect(isSshWorkspacePath([{ ...workspace, meta: undefined }], "/srv/project")).toBe(false);
  });
});
