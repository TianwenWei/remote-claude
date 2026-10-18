import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { ipc, type SshConnectionProbe, type SshHostKeyInspection, type Workspace } from "@/lib/ipc";
import i18n from "@/lib/i18n";
import { pickFile } from "@/lib/platform";
import { SshWorkspacesCard } from "./SshWorkspacesCard";

const chat = vi.hoisted(() => ({ startNewChat: vi.fn(), active: null as { workspacePath: string } | null }));

vi.mock("@/lib/transport", () => ({ isWeb: false, invoke: vi.fn(), listen: vi.fn() }));
vi.mock("@/lib/ipc", async (importOriginal) => ({
  ...await importOriginal<typeof import("@/lib/ipc")>(),
  ipc: { inspectSshHostKey: vi.fn(), trustSshHostKey: vi.fn(), probeSshConnection: vi.fn(), addSshWorkspace: vi.fn(), createSshWorkspace: vi.fn() },
}));
vi.mock("@/lib/platform", () => ({ pickFile: vi.fn() }));
vi.mock("@/features/chat/store", () => ({ useChatStore: { getState: () => chat } }));
vi.mock("@/features/chat/center-surfaces", () => ({ dismissCenterSurfaces: vi.fn() }));
Object.assign(globalThis, { IS_REACT_ACT_ENVIRONMENT: true });

const knownHost: SshHostKeyInspection = { status: "known", keys: [{ algorithm: "ssh-ed25519", fingerprint: "SHA256:verified" }], knownFingerprints: ["SHA256:verified"] };
const probe: SshConnectionProbe = {
  dependencies: { bash: { available: true, version: "5.2" }, python: { available: true, version: "3.12" }, git: { available: true, version: "2.45" }, claude: { available: true, version: "2.1.0" } },
  claudeAuth: { status: "authenticated", method: "oauth_token" },
  home: "/home/developer", claudeAvailable: true, claudeVersion: "2.1.0",
  canonicalPath: "/home/developer/projects", directories: [{ name: "demo", path: "/home/developer/projects/demo" }] };
let container: HTMLDivElement;
let root: Root;
let registered: ReturnType<typeof vi.fn>;

beforeEach(async () => {
  vi.resetAllMocks();
  vi.mocked(ipc.inspectSshHostKey).mockResolvedValue(knownHost);
  chat.active = null;
  window.location.hash = "#/settings?page=workspaces";
  container = document.createElement("div");
  document.body.appendChild(container);
  root = createRoot(container);
  registered = vi.fn(async () => {});
  await act(async () => { root.render(<SshWorkspacesCard workspaces={[]} onRegistered={registered} />); });
});
afterEach(async () => { await act(async () => root.unmount()); container.remove(); window.location.hash = ""; });

function button(key: string): HTMLButtonElement {
  const target = Array.from(container.querySelectorAll("button")).find((node) => node.textContent?.includes(i18n.t(key)));
  if (!target) throw new Error(`Missing button ${key}`);
  return target;
}
async function click(key: string) { await act(async () => { button(key).click(); }); }
async function edit(key: string, value: string) {
  const label = Array.from(container.querySelectorAll("label")).find((node) => node.textContent?.includes(i18n.t(key)));
  const input = document.getElementById(label!.htmlFor) as HTMLInputElement;
  await act(async () => {
    Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, "value")!.set!.call(input, value);
    input.dispatchEvent(new Event("input", { bubbles: true }));
  });
}

async function configureConnection() {
  await edit("settings.sshHost", "server.example");
  await edit("settings.sshUser", "developer");
}

function input(key: string): HTMLInputElement {
  const label = Array.from(container.querySelectorAll("label")).find((node) => node.textContent?.includes(i18n.t(key)));
  return document.getElementById(label!.htmlFor) as HTMLInputElement;
}

describe("native SSH connection card", () => {
  it("starts a new profile with empty credentials and the server home, without contacting a personal host", async () => {
    expect(input("settings.sshHost").value).toBe("");
    expect(input("settings.sshUser").value).toBe("");
    expect(input("settings.sshIdentityFile").value).toBe("");
    expect(input("settings.sshPort").value).toBe("22");
    expect(input("settings.sshProjectPath").value).toBe("~");
    expect(button("settings.sshTestConnection").disabled).toBe(true);
    expect(container.textContent).toContain(i18n.t("settings.sshPrepareServer"));
    await click("settings.sshTestConnection");
    expect(ipc.inspectSshHostKey).not.toHaveBeenCalled();
    expect(ipc.probeSshConnection).not.toHaveBeenCalled();
  });

  it("selects a local private-key path without reading its contents and invalidates the previous probe", async () => {
    await configureConnection();
    vi.mocked(ipc.probeSshConnection).mockResolvedValue(probe);
    await click("settings.sshTestConnection");
    expect(button("settings.sshAddProject").disabled).toBe(false);
    vi.mocked(pickFile).mockResolvedValue("/Users/friend/.ssh/trial_ed25519");
    await click("settings.sshChooseIdentity");
    expect(pickFile).toHaveBeenCalledExactlyOnceWith(i18n.t("settings.sshChooseIdentity"), []);
    expect(input("settings.sshIdentityFile").value).toBe("/Users/friend/.ssh/trial_ed25519");
    expect(button("settings.sshAddProject").disabled).toBe(true);
    await click("settings.sshTestConnection");
    expect(ipc.probeSshConnection).toHaveBeenLastCalledWith({ host: "server.example", user: "developer", port: 22,
      identityFile: "/Users/friend/.ssh/trial_ed25519" }, probe.canonicalPath);
  });

  it("preserves a verified connection when the file picker is cancelled and rejects non-absolute key paths", async () => {
    await configureConnection();
    vi.mocked(ipc.probeSshConnection).mockResolvedValue(probe);
    await click("settings.sshTestConnection");
    vi.mocked(pickFile).mockResolvedValue(null);
    await click("settings.sshChooseIdentity");
    expect(input("settings.sshIdentityFile").value).toBe("");
    expect(button("settings.sshAddProject").disabled).toBe(false);
    vi.mocked(pickFile).mockResolvedValue("relative/private-key");
    await click("settings.sshChooseIdentity");
    expect(input("settings.sshIdentityFile").value).toBe("");
    expect(container.querySelector('[role="alert"]')?.textContent).toContain(i18n.t("settings.sshIdentityInvalid"));
    await edit("settings.sshIdentityFile", "~/.ssh/id_ed25519");
    expect(button("settings.sshTestConnection").disabled).toBe(true);
  });

  it("requires explicit external verification of the selected host fingerprint before trusting and probing", async () => {
    await configureConnection();
    const discovered: SshHostKeyInspection = { status: "new", knownFingerprints: [], keys: [
      { algorithm: "ssh-ed25519", fingerprint: "SHA256:first" },
      { algorithm: "ecdsa-sha2-nistp256", fingerprint: "SHA256:second" },
    ] };
    vi.mocked(ipc.inspectSshHostKey).mockResolvedValue(discovered);
    vi.mocked(ipc.probeSshConnection).mockResolvedValue(probe);
    await click("settings.sshTestConnection");
    expect(ipc.inspectSshHostKey).toHaveBeenCalledExactlyOnceWith("server.example", 22, "developer");
    expect(container.textContent).toContain("SHA256:first");
    expect(container.textContent).toContain("SHA256:second");
    expect(ipc.probeSshConnection).not.toHaveBeenCalled();
    expect(button("settings.sshTrustHost").disabled).toBe(true);
    await act(async () => { container.querySelector<HTMLInputElement>('input[type="checkbox"]')!.click(); });
    expect(button("settings.sshTrustHost").disabled).toBe(false);
    await act(async () => { container.querySelectorAll<HTMLInputElement>('input[type="radio"]')[1].click(); });
    expect(button("settings.sshTrustHost").disabled).toBe(true);
    await act(async () => { container.querySelector<HTMLInputElement>('input[type="checkbox"]')!.click(); });
    let finish!: (inspection: SshHostKeyInspection) => void;
    vi.mocked(ipc.trustSshHostKey).mockImplementation(() => new Promise((resolve) => { finish = resolve; }));
    await act(async () => { const trust = button("settings.sshTrustHost"); trust.click(); trust.click(); });
    expect(ipc.trustSshHostKey).toHaveBeenCalledExactlyOnceWith("server.example", 22, "SHA256:second", "developer");
    expect(ipc.probeSshConnection).not.toHaveBeenCalled();
    await act(async () => { finish(knownHost); });
    expect(ipc.probeSshConnection).toHaveBeenCalledExactlyOnceWith({ host: "server.example", user: "developer", port: 22, identityFile: null }, "~");
    expect(button("settings.sshAddProject").disabled).toBe(false);
  });

  it("blocks a changed host key and never silently overwrites trust or starts authentication", async () => {
    await configureConnection();
    vi.mocked(ipc.inspectSshHostKey).mockResolvedValue({ ...knownHost, status: "changed", knownFingerprints: ["SHA256:old"] });
    await click("settings.sshTestConnection");
    expect(container.querySelector('[role="alert"]')?.textContent).toBe(i18n.t("settings.sshHostChanged"));
    expect(container.textContent).toContain("SHA256:old");
    expect(container.textContent).toContain("SHA256:verified");
    expect(container.querySelector('input[type="checkbox"]')).toBeNull();
    expect(button("settings.sshAddProject").disabled).toBe(true);
    expect(ipc.trustSshHostKey).not.toHaveBeenCalled();
    expect(ipc.probeSshConnection).not.toHaveBeenCalled();
  });

  it("keeps an unsuccessful trust visible and rechecks a changed fingerprint before any directory probe", async () => {
    await configureConnection();
    vi.mocked(ipc.inspectSshHostKey).mockResolvedValue({ ...knownHost, status: "new", knownFingerprints: [] });
    await click("settings.sshTestConnection");
    await act(async () => { container.querySelector<HTMLInputElement>('input[type="checkbox"]')!.click(); });
    vi.mocked(ipc.trustSshHostKey).mockRejectedValueOnce(new Error("Fingerprint changed during verification"));
    await click("settings.sshTrustHost");
    expect(container.querySelector('[role="alert"]')?.textContent).toContain("Fingerprint changed during verification");
    expect(ipc.probeSshConnection).not.toHaveBeenCalled();
    vi.mocked(ipc.trustSshHostKey).mockResolvedValue({ ...knownHost, status: "changed" });
    await click("settings.sshTrustHost");
    expect(ipc.probeSshConnection).not.toHaveBeenCalled();
    expect(button("settings.sshAddProject").disabled).toBe(true);
  });

  it("requires remote Claude authentication and complete dependencies without hiding browsable folders", async () => {
    await configureConnection();
    await edit("settings.sshProjectName", "ready-later");
    for (const status of ["unauthenticated", "unknown"] as const) {
      vi.mocked(ipc.probeSshConnection).mockResolvedValue({ ...probe, claudeAuth: { status, method: null } });
      await click("settings.sshTestConnection");
      expect(container.textContent).toContain(i18n.t(status === "unauthenticated" ? "settings.sshClaudeUnauthenticated" : "settings.sshClaudeAuthUnknown"));
      expect(container.textContent).toContain("demo");
      expect(button("settings.sshAddProject").disabled).toBe(true);
      expect(button("settings.sshCreateProject").disabled).toBe(true);
    }
    vi.mocked(ipc.probeSshConnection).mockResolvedValue({ ...probe, dependencies: { ...probe.dependencies, python: { available: false, version: null } } });
    await click("settings.sshTestConnection");
    expect(container.textContent).toContain(i18n.t("settings.sshDependenciesMissing", { names: "python" }));
    expect(button("settings.sshAddProject").disabled).toBe(true);
    expect(button("settings.sshCreateProject").disabled).toBe(true);
    expect(ipc.addSshWorkspace).not.toHaveBeenCalled();
    expect(ipc.createSshWorkspace).not.toHaveBeenCalled();
  });

  it("allows adding a remote Claude project when optional Git is missing and explains its limited capability", async () => {
    await configureConnection();
    vi.mocked(ipc.probeSshConnection).mockResolvedValue({ ...probe, dependencies: { ...probe.dependencies, git: { available: false, version: null } } });
    await click("settings.sshTestConnection");
    expect(container.textContent).toContain(i18n.t("settings.sshGitMissing"));
    expect(button("settings.sshAddProject").disabled).toBe(false);
    const workspace = { id: "without-git", name: "project", path: probe.canonicalPath } as Workspace;
    vi.mocked(ipc.addSshWorkspace).mockResolvedValue(workspace);
    await click("settings.sshAddProject");
    expect(ipc.addSshWorkspace).toHaveBeenCalledExactlyOnceWith({ host: "server.example", user: "developer", port: 22, identityFile: null }, probe.canonicalPath);
    expect(registered).toHaveBeenCalledExactlyOnceWith(workspace);
  });

  it("does not start a directory probe when host inspection finishes after unmount", async () => {
    await configureConnection();
    let finish!: (inspection: SshHostKeyInspection) => void;
    vi.mocked(ipc.inspectSshHostKey).mockImplementation(() => new Promise((resolve) => { finish = resolve; }));
    await click("settings.sshTestConnection");
    await act(async () => root.unmount());
    await act(async () => finish(knownHost));
    expect(ipc.probeSshConnection).not.toHaveBeenCalled();
  });

  it("requires a successful remote Claude probe before adding a selected server directory", async () => {
    await configureConnection();
    expect(button("settings.sshAddProject").disabled).toBe(true);
    vi.mocked(ipc.probeSshConnection).mockResolvedValue(probe);
    await click("settings.sshTestConnection");
    expect(button("settings.sshAddProject").disabled).toBe(false);
    expect(container.textContent).toContain("2.1.0");
    vi.mocked(ipc.probeSshConnection).mockResolvedValue({ ...probe, canonicalPath: "/home/developer/projects/demo", directories: [] });
    await act(async () => { Array.from(container.querySelectorAll("button")).find((node) => node.textContent === "demo")!.click(); });
    vi.mocked(ipc.addSshWorkspace).mockResolvedValue({ id: "remote", name: "demo", path: "/home/developer/projects/demo" } as Workspace);
    await click("settings.sshAddProject");
    expect(ipc.addSshWorkspace).toHaveBeenCalledExactlyOnceWith({ host: "server.example", user: "developer", port: 22, identityFile: null }, "/home/developer/projects/demo");
    expect(registered).toHaveBeenCalledOnce();
    expect(container.textContent).toContain(i18n.t("settings.sshWorkspaceAdded", { name: "demo" }));
  });

  it("invalidates old probe results when credentials or project directory change", async () => {
    await configureConnection();
    vi.mocked(ipc.probeSshConnection).mockResolvedValue(probe);
    await click("settings.sshTestConnection");
    await edit("settings.sshHost", "other-host");
    expect(button("settings.sshAddProject").disabled).toBe(true);
    expect(container.textContent).not.toContain("2.1.0");
    await edit("settings.sshPort", "65536");
    expect(button("settings.sshTestConnection").disabled).toBe(true);
  });

  it("shows SSH failures and remote CLI absence without a false ready state", async () => {
    await configureConnection();
    vi.mocked(ipc.probeSshConnection).mockRejectedValue(new Error("Host key verification failed"));
    await click("settings.sshTestConnection");
    expect(container.querySelector('[role="alert"]')?.textContent).toContain("Host key verification failed");
    expect(button("settings.sshAddProject").disabled).toBe(true);
    vi.mocked(ipc.probeSshConnection).mockResolvedValue({ ...probe, claudeAvailable: false, claudeVersion: null });
    await click("settings.sshTestConnection");
    expect(container.querySelector('[role="alert"]')).toBeNull();
    expect(container.textContent).toContain(i18n.t("settings.sshClaudeMissing"));
    expect(button("settings.sshAddProject").disabled).toBe(true);
    expect(ipc.addSshWorkspace).not.toHaveBeenCalled();
  });

  it("prevents duplicate registration and leaves failures visible without success text", async () => {
    await configureConnection();
    vi.mocked(ipc.probeSshConnection).mockResolvedValue(probe);
    await click("settings.sshTestConnection");
    let reject!: (error: Error) => void;
    vi.mocked(ipc.addSshWorkspace).mockImplementation(() => new Promise((_resolve, fail) => { reject = fail; }));
    await act(async () => { const add = button("settings.sshAddProject"); add.click(); add.click(); });
    expect(ipc.addSshWorkspace).toHaveBeenCalledOnce();
    await act(async () => { reject(new Error("Project is registered on another host")); });
    expect(registered).not.toHaveBeenCalled();
    expect(container.querySelector('[role="alert"]')?.textContent).toContain("Project is registered on another host");
    expect(container.textContent).not.toContain(i18n.t("settings.sshWorkspaceAdded", { name: "demo" }));
  });

  it("ignores a pending response after the card unmounts", async () => {
    await configureConnection();
    let finish!: (value: SshConnectionProbe) => void;
    vi.mocked(ipc.probeSshConnection).mockImplementation(() => new Promise((resolve) => { finish = resolve; }));
    await click("settings.sshTestConnection");
    await act(async () => { root.unmount(); });
    await act(async () => { finish(probe); });
    expect(container.textContent).toBe("");
    expect(ipc.addSshWorkspace).not.toHaveBeenCalled();
  });

  it("creates a new server folder inside the browsed parent, registers it and opens its remote chat", async () => {
    await configureConnection();
    await edit("settings.sshProjectName", " 新项目 ");
    expect(button("settings.sshCreateProject").disabled).toBe(true);
    vi.mocked(ipc.probeSshConnection).mockResolvedValue(probe);
    await click("settings.sshTestConnection");
    vi.mocked(ipc.probeSshConnection).mockResolvedValue({ ...probe, canonicalPath: "/home/developer/projects/demo", directories: [] });
    await act(async () => { Array.from(container.querySelectorAll("button")).find((node) => node.textContent === "demo")!.click(); });
    const workspace = { id: "new", name: "新项目", path: "/home/developer/projects/demo/新项目" } as Workspace;
    vi.mocked(ipc.createSshWorkspace).mockResolvedValue(workspace);
    await click("settings.sshCreateProject");
    expect(ipc.createSshWorkspace).toHaveBeenCalledExactlyOnceWith({ host: "server.example", user: "developer", port: 22, identityFile: null }, "/home/developer/projects/demo", "新项目");
    expect(ipc.addSshWorkspace).not.toHaveBeenCalled();
    expect(registered).toHaveBeenCalledExactlyOnceWith(workspace);
    expect(chat.startNewChat).toHaveBeenCalledExactlyOnceWith(workspace.path);
    expect(window.location.hash).toBe("#/");
  });

  it("requires one folder name and invalidates creation when the selected parent changes", async () => {
    await configureConnection();
    vi.mocked(ipc.probeSshConnection).mockResolvedValue(probe);
    await click("settings.sshTestConnection");
    for (const invalid of ["", ".", "..", "nested/path", "nested\\path", "bad\u0000name", "bad\u007fname", "bad\u0085name", "bad\tname"]) {
      await edit("settings.sshProjectName", invalid);
      expect(button("settings.sshCreateProject").disabled).toBe(true);
      await click("settings.sshCreateProject");
      expect(ipc.createSshWorkspace).not.toHaveBeenCalled();
    }
    await edit("settings.sshProjectName", "my.project");
    expect(button("settings.sshCreateProject").disabled).toBe(false);
    await edit("settings.sshProjectPath", "/srv/other");
    expect(button("settings.sshCreateProject").disabled).toBe(true);
    expect(ipc.createSshWorkspace).not.toHaveBeenCalled();
  });

  it("keeps a failed creation editable, prevents duplicate creates and preserves existing directories", async () => {
    await configureConnection();
    vi.mocked(ipc.probeSshConnection).mockResolvedValue(probe);
    await click("settings.sshTestConnection");
    await edit("settings.sshProjectName", "existing");
    let fail!: (error: Error) => void;
    vi.mocked(ipc.createSshWorkspace).mockImplementationOnce(() => new Promise((_resolve, reject) => { fail = reject; }));
    await act(async () => { const create = button("settings.sshCreateProject"); create.click(); create.click(); });
    expect(ipc.createSshWorkspace).toHaveBeenCalledOnce();
    expect(input("settings.sshProjectName").disabled).toBe(true);
    await act(async () => { fail(new Error("Directory already exists")); });
    expect(input("settings.sshProjectName").value).toBe("existing");
    expect(button("settings.sshCreateProject").disabled).toBe(false);
    expect(container.querySelector('[role="alert"]')?.textContent).toContain("Directory already exists");
    expect(chat.startNewChat).not.toHaveBeenCalled();
    expect(registered).not.toHaveBeenCalled();
  });

  it("reports an already-created project accurately if the subsequent refresh fails", async () => {
    await configureConnection();
    vi.mocked(ipc.probeSshConnection).mockResolvedValue(probe);
    await click("settings.sshTestConnection");
    await edit("settings.sshProjectName", "created");
    vi.mocked(ipc.createSshWorkspace).mockResolvedValue({ id: "new", name: "created", path: "/home/developer/projects/created" } as Workspace);
    registered.mockRejectedValueOnce(new Error("History refresh failed"));
    await click("settings.sshCreateProject");
    expect(container.querySelector('[role="alert"]')?.textContent).toBe(i18n.t("settings.sshCreatedRefreshFailed", { name: "created", error: "History refresh failed" }));
    expect(input("settings.sshProjectName").value).toBe("");
    expect(chat.startNewChat).not.toHaveBeenCalled();
    expect(ipc.createSshWorkspace).toHaveBeenCalledOnce();
  });

  it("seeds the new-project entry from the active remote host and its parent directory", async () => {
    await configureConnection();
    await act(async () => root.unmount());
    root = createRoot(container);
    window.location.hash = "#/settings?page=workspaces&action=new-ssh-project";
    chat.active = { workspacePath: "/srv/projects/welcome" };
    const workspace = { id: "active", name: "welcome", path: chat.active.workspacePath, meta: { ssh: {
      host: "remote.example", user: "dev", port: 2222, identityFile: null, workspace: chat.active.workspacePath,
      enginePaths: { claude: "/usr/bin/claude" },
    } } } as unknown as Workspace;
    await act(async () => root.render(<SshWorkspacesCard workspaces={[workspace]} onRegistered={registered} />));
    expect(input("settings.sshHost").value).toBe("remote.example");
    expect(input("settings.sshUser").value).toBe("dev");
    expect(input("settings.sshPort").value).toBe("2222");
    expect(input("settings.sshIdentityFile").value).toBe("");
    expect(input("settings.sshProjectPath").value).toBe("/srv/projects");
    vi.mocked(ipc.probeSshConnection).mockResolvedValue({ ...probe, canonicalPath: "/srv/projects", directories: [] });
    await click("settings.sshTestConnection");
    await edit("settings.sshProjectName", "fresh");
    vi.mocked(ipc.createSshWorkspace).mockResolvedValue({ ...workspace, name: "fresh", path: "/srv/projects/fresh" });
    await click("settings.sshCreateProject");
    expect(ipc.createSshWorkspace).toHaveBeenCalledExactlyOnceWith({ host: "remote.example", user: "dev", port: 2222, identityFile: null }, "/srv/projects", "fresh");
  });

  it("keeps the active workspace connection and identity rather than replacing them with empty onboarding values", async () => {
    await act(async () => root.unmount());
    root = createRoot(container);
    chat.active = { workspacePath: "/srv/current" };
    const workspace = { id: "active", name: "current", path: "/srv/current", meta: { ssh: {
      host: "existing.example", user: "existing", port: 2200, identityFile: "/Users/friend/.ssh/existing", workspace: "/srv/current",
      enginePaths: { claude: "/usr/bin/claude" },
    } } } as unknown as Workspace;
    const other = { ...workspace, id: "other", path: "/srv/other", meta: { ssh: { ...workspace.meta!.ssh as Record<string, unknown>, host: "other.example", workspace: "/srv/other" } } };
    await act(async () => root.render(<SshWorkspacesCard workspaces={[other, workspace]} onRegistered={registered} />));
    expect(input("settings.sshHost").value).toBe("existing.example");
    expect(input("settings.sshUser").value).toBe("existing");
    expect(input("settings.sshPort").value).toBe("2200");
    expect(input("settings.sshIdentityFile").value).toBe("/Users/friend/.ssh/existing");
    expect(input("settings.sshProjectPath").value).toBe("/srv/current");
    vi.mocked(ipc.probeSshConnection).mockResolvedValue(probe);
    await click("settings.sshTestConnection");
    expect(ipc.probeSshConnection).toHaveBeenCalledExactlyOnceWith({ host: "existing.example", user: "existing", port: 2200, identityFile: "/Users/friend/.ssh/existing" }, "/srv/current");
  });
});
