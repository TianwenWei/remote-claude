import { beforeEach, describe, expect, it, vi } from "vitest";
import { createStore } from "zustand/vanilla";
import { ipc, type SessionMeta, type Workspace } from "@/lib/ipc";
import { listSshSessionMetas } from "./ssh-sessions";
import { cacheSshSessionMetas, cachedSshSessionMetas, removeCachedSshSessionMeta, SSH_SESSION_CATALOG_KEY } from "./ssh-session-catalog";
import { createTabActions } from "./tabs";
import type { ChatStore } from "./types";

vi.mock("@/lib/ipc", async (importOriginal) => ({
  ...await importOriginal<typeof import("@/lib/ipc")>(),
  ipc: { listSshSessions: vi.fn() },
}));

const workspace: Workspace = { id: "remote", name: "project", path: "/home/cc/project", lastOpenedAt: null, sortOrder: null, groupId: null, meta: { ssh: {
  host: "vps", user: "cc", port: 22, identityFile: null, workspace: "/home/cc/project",
  enginePaths: { claude: "/home/cc/.local/bin/claude" },
} } };
const session: SessionMeta = { engine: "claude", sessionId: "id", workspacePath: workspace.path, remote: true,
  remotePath: "/home/cc/.claude/projects/project/id.jsonl", filePath: "/home/cc/.claude/projects/project/id.jsonl",
  title: "Saved conversation", preview: "Private transcript preview", fileSize: 2650000, fileMtimeMs: 1,
  createdAt: 1, updatedAt: 2, messageCount: 100, pinned: false, customTitle: null };

beforeEach(() => { vi.clearAllMocks(); localStorage.clear(); });

describe("SSH workspace history", () => {
  it("queries registered SSH projects and keeps their remote identity", async () => {
    vi.mocked(ipc.listSshSessions).mockResolvedValue([session]);
    const result = await listSshSessionMetas([workspace, { path: "/local" } as Workspace], []);
    expect(ipc.listSshSessions).toHaveBeenCalledExactlyOnceWith(workspace.path);
    expect(result).toEqual({ sessions: [session], errors: [] });
  });

  it("coalesces overlapping startup and refresh batches for three SSH projects", async () => {
    const projects = [workspace, { ...workspace, id: "second", path: "/home/cc/second" },
      { ...workspace, id: "third", path: "/home/cc/third" }];
    const pending = projects.map(() => Promise.withResolvers<SessionMeta[]>());
    vi.mocked(ipc.listSshSessions).mockImplementation((path) => pending[projects.findIndex((project) => project.path === path)].promise);
    const startup = listSshSessionMetas(projects, []);
    const refresh = listSshSessionMetas([...projects].reverse(), []);
    expect(ipc.listSshSessions).toHaveBeenCalledTimes(3);
    pending.forEach((request, index) => request.resolve([{ ...session, workspacePath: projects[index].path }]));
    const results = await Promise.all([startup, refresh]);
    expect(results[0].sessions).toHaveLength(3);
    expect(results[1].sessions).toHaveLength(3);
    await listSshSessionMetas(projects, []);
    expect(ipc.listSshSessions).toHaveBeenCalledTimes(6);
  });

  it.each([{ host: "other-vps" }, { user: "other-user" }, { port: 2222 }, { identityFile: "/different/key" },
    { workspace: "/different/remote/root" }])("does not share an in-flight read across SSH sources %j", async (change) => {
    const changed = { ...workspace, meta: { ssh: { ...(workspace.meta!.ssh as object), ...change } } } as Workspace;
    const pending = Promise.withResolvers<SessionMeta[]>();
    vi.mocked(ipc.listSshSessions).mockReturnValue(pending.promise);
    const first = listSshSessionMetas([workspace], []);
    const second = listSshSessionMetas([changed], []);
    expect(ipc.listSshSessions).toHaveBeenCalledTimes(2);
    pending.resolve([]);
    await Promise.all([first, second]);
  });

  it("shares only the network read while keeping each caller's offline fallback", async () => {
    const pending = Promise.withResolvers<SessionMeta[]>();
    vi.mocked(ipc.listSshSessions).mockReturnValue(pending.promise);
    const previous = { ...session, title: "Already loaded" };
    cacheSshSessionMetas([workspace], workspace, [session]);
    const loaded = listSshSessionMetas([workspace], [previous]);
    const restarted = listSshSessionMetas([workspace], []);
    pending.reject(new Error("Connection closed"));
    const [first, second] = await Promise.all([loaded, restarted]);
    expect(ipc.listSshSessions).toHaveBeenCalledTimes(1);
    expect(first.sessions[0].title).toBe("Already loaded");
    expect(second.sessions[0].preview).toBe("");
    vi.mocked(ipc.listSshSessions).mockResolvedValueOnce([]);
    expect((await listSshSessionMetas([workspace], [])).errors).toEqual([]);
    expect(ipc.listSshSessions).toHaveBeenCalledTimes(2);
  });

  it("retains unloaded history after SSH failure and reports the project and cause", async () => {
    vi.mocked(ipc.listSshSessions).mockRejectedValue(new Error("Permission denied (publickey)"));
    const other = { ...session, workspacePath: "/other" };
    const result = await listSshSessionMetas([workspace], [session, other]);
    expect(result.sessions).toEqual([session]);
    expect(result.errors).toEqual([{ name: "project", error: "Permission denied (publickey)" }]);
  });

  it("reopens a persisted thin catalog after restart even when SSH listing fails", async () => {
    vi.mocked(ipc.listSshSessions).mockResolvedValueOnce([session]);
    await listSshSessionMetas([workspace], []);
    vi.mocked(ipc.listSshSessions).mockRejectedValueOnce(new Error("Offline"));
    const result = await listSshSessionMetas([workspace], []);
    expect(result.sessions[0]).toMatchObject({ sessionId: session.sessionId, remote: true, remotePath: session.remotePath, preview: "" });
    expect(result.errors[0].error).toBe("Offline");
    const persisted = localStorage.getItem(SSH_SESSION_CATALOG_KEY)!;
    expect(persisted).not.toContain(session.preview);
    expect(persisted).not.toContain("identityFile");
  });

  it.each([{ host: "other-vps" }, { user: "other-user" }, { port: 2222 }])("isolates the catalog by SSH connection %j", (change) => {
    cacheSshSessionMetas([workspace], workspace, [session]);
    const changed = { ...workspace, meta: { ssh: { ...(workspace.meta!.ssh as object), ...change } } } as Workspace;
    expect(cachedSshSessionMetas([changed])).toEqual([]);
  });

  it("keeps a connection catalog when only the private key path changes without persisting credentials", () => {
    const original = { ...workspace, meta: { ssh: { ...(workspace.meta!.ssh as object), identityFile: "/private/key-original" } } } as Workspace;
    cacheSshSessionMetas([original], original, [session]);
    const changed = { ...workspace, meta: { ssh: { ...(workspace.meta!.ssh as object), identityFile: "/private/key-new" } } } as Workspace;
    expect(cachedSshSessionMetas([changed])[0].sessionId).toBe(session.sessionId);
    expect(localStorage.getItem(SSH_SESSION_CATALOG_KEY)).not.toContain("/private/");
  });

  it("prunes a successful empty listing and excluded archived rows instead of reviving them offline", async () => {
    cacheSshSessionMetas([workspace], workspace, [session]);
    vi.mocked(ipc.listSshSessions).mockResolvedValueOnce([]);
    await listSshSessionMetas([workspace], [session]);
    expect(cachedSshSessionMetas([workspace])).toEqual([]);
    vi.mocked(ipc.listSshSessions).mockResolvedValueOnce([session]);
    await listSshSessionMetas([workspace], [], { "claude/id": true });
    expect(cachedSshSessionMetas([workspace])).toEqual([]);
    vi.mocked(ipc.listSshSessions).mockRejectedValueOnce(new Error("Offline"));
    expect((await listSshSessionMetas([workspace], [])).sessions).toEqual([]);
  });

  it("bounds metadata and removes deleted sessions and removed workspace partitions", () => {
    const rows = Array.from({ length: 600 }, (_, index) => ({ ...session, sessionId: `id-${index}`, updatedAt: index }));
    cacheSshSessionMetas([workspace], workspace, rows);
    expect(cachedSshSessionMetas([workspace])).toHaveLength(500);
    removeCachedSshSessionMeta([workspace], "claude", "id-599", workspace.path);
    expect(cachedSshSessionMetas([workspace]).some((meta) => meta.sessionId === "id-599")).toBe(false);
    cachedSshSessionMetas([]);
    expect(JSON.parse(localStorage.getItem(SSH_SESSION_CATALOG_KEY)!)).toEqual({});
  });

  it("does not repersist a deleted session when an older in-flight listing finishes later", async () => {
    cacheSshSessionMetas([workspace], workspace, [session]);
    const pending = Promise.withResolvers<SessionMeta[]>();
    vi.mocked(ipc.listSshSessions).mockReturnValueOnce(pending.promise);
    const listing = listSshSessionMetas([workspace], [session]);
    removeCachedSshSessionMeta([workspace], session.engine, session.sessionId, workspace.path);
    pending.resolve([session]);
    const result = await listing;
    expect(result.sessions).toEqual([]);
    expect(cachedSshSessionMetas([workspace])).toEqual([]);
  });

  it("starts a new revision after delete and keeps sharing it when the older read settles", async () => {
    cacheSshSessionMetas([workspace], workspace, [session]);
    const oldRead = Promise.withResolvers<SessionMeta[]>();
    const newRead = Promise.withResolvers<SessionMeta[]>();
    vi.mocked(ipc.listSshSessions).mockReturnValueOnce(oldRead.promise).mockReturnValueOnce(newRead.promise);
    const older = listSshSessionMetas([workspace], [session]);
    removeCachedSshSessionMeta([workspace], session.engine, session.sessionId, workspace.path);
    const newer = listSshSessionMetas([workspace], []);
    expect(ipc.listSshSessions).toHaveBeenCalledTimes(2);
    oldRead.resolve([session]);
    expect((await older).sessions).toEqual([]);
    const joined = listSshSessionMetas([workspace], []);
    expect(ipc.listSshSessions).toHaveBeenCalledTimes(2);
    newRead.resolve([]);
    await Promise.all([newer, joined]);
    expect(cachedSshSessionMetas([workspace])).toEqual([]);
  });

  it("ignores corrupt or invalid persisted routes", () => {
    localStorage.setItem(SSH_SESSION_CATALOG_KEY, "broken-json");
    expect(cachedSshSessionMetas([workspace])).toEqual([]);
    const key = JSON.stringify(["vps", "cc", 22, workspace.path]);
    localStorage.setItem(SSH_SESSION_CATALOG_KEY, JSON.stringify({ [key]: [{ ...session, remotePath: "relative/transcript" }] }));
    expect(cachedSshSessionMetas([workspace])).toEqual([]);
  });

  it("starts remote chats with Claude while the previous local engine is Codex", () => {
    const store = createStore<ChatStore>(() => ({ workspaces: [workspace], activeEngine: "codex",
      openTabs: [], active: null, pendingRuntimeSwitch: null }) as unknown as ChatStore);
    const actions = createTabActions({ set: store.setState, get: store.getState });
    actions.startNewChat(workspace.path);
    expect(store.getState().active).toEqual({ engine: "claude", workspacePath: workspace.path, sessionId: null });
    expect(store.getState().activeEngine).toBe("claude");
    actions.startNewChat(workspace.path);
    expect(store.getState().openTabs).toHaveLength(1);
  });
});
