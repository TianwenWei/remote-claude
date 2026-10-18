import { describe, expect, it } from "vitest";
import { filterEngineOptions, orderEngineOptions, sshWorkspaceAllowedEngines } from "./engine-options";
import type { EngineInfo, Workspace } from "@/lib/ipc";

function engine(id: string, over: Partial<EngineInfo> = {}): EngineInfo {
  return { id, enabled: true, available: true, ...over } as EngineInfo;
}

const t = (key: string) => key;

describe("filterEngineOptions", () => {
  it("drops disabled engines entirely", () => {
    const out = filterEngineOptions(
      [engine("omp"), engine("claude", { enabled: false })],
      null,
      t,
    );
    expect(out.map((o) => o.id)).toEqual(["omp"]);
  });

  it("drops engines the local binary probe marks uninstalled", () => {
    const out = filterEngineOptions(
      [engine("omp", { available: false }), engine("claude")],
      null,
      t,
    );
    expect(out.map((o) => o.id)).toEqual(["claude"]);
  });

  it("WSL 工作区:只列发行版内探到的引擎,可用态按探针而非本机", () => {
    const out = filterEngineOptions(
      // 本机都没装(available=false),但发行版里探到 omp
      [engine("omp", { available: false }), engine("claude", { available: false })],
      ["omp"],
      t,
    );
    expect(out.map((o) => o.id)).toEqual(["omp"]);
    expect(out[0]?.available).toBe(true);
    expect(out[0]?.disabled).toBe(false);
  });

  it("uses the SSH host's Claude discovery when this Mac has no Claude CLI", () => {
    const workspace = { meta: { ssh: { host: "vps", user: "cc", port: 22, identityFile: null,
      workspace: "/home/cc/project", enginePaths: { claude: "/home/cc/.local/bin/claude" } } } } as unknown as Workspace;
    const allowed = sshWorkspaceAllowedEngines(workspace);
    expect(allowed).toEqual(["claude"]);
    expect(filterEngineOptions([engine("codex"), engine("claude", { available: false })], allowed, t))
      .toMatchObject([{ id: "claude", available: true, disabled: false }]);
    workspace.meta!.ssh = { ...(workspace.meta!.ssh as object), enginePaths: {} };
    expect(sshWorkspaceAllowedEngines(workspace)).toEqual([]);
    expect(filterEngineOptions([engine("codex")], sshWorkspaceAllowedEngines(workspace), t)).toEqual([]);
    expect(sshWorkspaceAllowedEngines(undefined)).toBeNull();
  });
});

describe("orderEngineOptions", () => {
  const options = (ids: string[]) =>
    filterEngineOptions(ids.map((id) => engine(id)), null, t);

  it("follows the settings rail drag order, keyed cli:<id>", () => {
    const out = orderEngineOptions(options(["claude", "omp", "kimi"]), [
      "cli:kimi",
      "cli:claude",
    ]);
    expect(out.map((o) => o.id)).toEqual(["kimi", "claude", "omp"]);
  });

  it("keeps registry order when no preference is stored", () => {
    const out = orderEngineOptions(options(["omp", "claude"]), []);
    expect(out.map((o) => o.id)).toEqual(["omp", "claude"]);
  });
});
