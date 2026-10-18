// Real production card, entirely fake native transport. No SSH, model request,
// credential read or saved app/browser state is involved.
import { useEffect, useState } from "react";
import { createRoot } from "react-dom/client";
import "../../src/index.css";
import i18n from "../../src/lib/i18n";
import { SshWorkspacesCard } from "../../src/features/settings/SshWorkspacesCard";
import { useChatStore } from "../../src/features/chat/store";
import type { SshConnectionInput, SshConnectionProbe, SshHostKeyInspection, Workspace } from "../../src/lib/ipc";

type Scenario = "ready" | "changed" | "missingPython" | "unauthenticated";
const fingerprint = "SHA256:DemoOnlyFingerprintVerifyOutsideThisFixture000";
const hostKeys = [{ algorithm: "ssh-ed25519", fingerprint }];
const listeners = new Set<() => void>();
const state = {
  scenario: "ready" as Scenario,
  trusted: false,
  reset: 0,
  calls: {} as Record<string, number>,
  workspaces: [] as Workspace[],
  opened: null as string | null,
};

function publish() { for (const listener of listeners) listener(); }
function record(command: string) {
  state.calls[command] = (state.calls[command] ?? 0) + 1;
  publish();
}

function inspection(): SshHostKeyInspection {
  return {
    status: state.scenario === "changed" ? "changed" : state.trusted ? "known" : "new",
    keys: hostKeys,
    knownFingerprints: state.scenario === "changed" ? ["SHA256:PreviouslySavedDemoFingerprint"] : state.trusted ? [fingerprint] : [],
  };
}

function canonicalPath(value: string) {
  const parts: string[] = [];
  for (const segment of (value === "~" || !value ? "/home/demo" : value).split("/")) {
    if (segment === "..") parts.pop();
    else if (segment && segment !== ".") parts.push(segment);
  }
  return `/${parts.join("/")}`;
}

function directoryProbe(path: string): SshConnectionProbe {
  const directory = canonicalPath(path);
  const children: Record<string, string[]> = { "/": ["home"], "/home": ["demo"], "/home/demo": ["projects"], "/home/demo/projects": ["welcome", "demo-app"] };
  return {
    home: "/home/demo", canonicalPath: directory, claudeAvailable: true, claudeVersion: "Demo CLI",
    dependencies: {
      bash: { available: true, version: "Demo Bash" },
      python: { available: state.scenario !== "missingPython", version: state.scenario === "missingPython" ? null : "Demo Python" },
      git: { available: true, version: "Demo Git" },
      claude: { available: true, version: "Demo CLI" },
    },
    claudeAuth: { status: state.scenario === "unauthenticated" ? "unauthenticated" : "authenticated", method: state.scenario === "unauthenticated" ? null : "claude.ai" },
    directories: (children[directory] ?? []).map((name) => ({ name, path: `${directory.replace(/\/$/, "")}/${name}` })),
  };
}

function assertReady() {
  if (!state.trusted || state.scenario !== "ready") throw new Error("模拟服务器尚未准备好，不能注册项目。");
}

function workspace(connection: SshConnectionInput, path: string): Workspace {
  return {
    id: `demo-${path}`, path, name: path.split("/").filter(Boolean).at(-1) ?? "/",
    lastOpenedAt: 0, sortOrder: 0, groupId: null,
    meta: { ssh: { ...connection, workspace: path, enginePaths: { claude: "/demo/bin/claude" } } },
  };
}

window.__TAURI_INTERNALS__!.invoke = async (command: string, rawArgs?: unknown) => {
  const args = rawArgs as Record<string, unknown> | undefined;
  record(command);
  switch (command) {
    case "inspect_ssh_host_key": return inspection();
    case "trust_ssh_host_key":
      if (state.scenario === "changed" || args?.expectedFingerprint !== fingerprint) throw new Error("模拟主机指纹冲突，未写入信任。");
      state.trusted = true;
      publish();
      return inspection();
    case "probe_ssh_connection":
      if (!state.trusted || state.scenario === "changed") throw new Error("模拟主机尚未信任。");
      return directoryProbe(String(args?.path ?? "~"));
    case "add_ssh_workspace":
      assertReady();
      return workspace(args?.connection as SshConnectionInput, String(args?.path));
    case "create_ssh_workspace":
      assertReady();
      return workspace(args?.connection as SshConnectionInput, `${String(args?.parentPath).replace(/\/$/, "")}/${String(args?.name)}`);
    case "plugin:dialog|open": return "/Users/demo/.ssh/demo_ed25519";
    case "plugin:event|listen":
    case "plugin:event|unlisten": return 1;
    default: throw new Error(`SSH fixture blocked unexpected IPC: ${command}`);
  }
};

// The real card may open a newly created project. Keep that navigation local to
// this fixture; no session initialization or file/Git query is needed here.
useChatStore.setState({ active: null, workspaces: [], startNewChat: (path) => { state.opened = path; record("fixture_open_project"); } });

function reset(scenario = state.scenario) {
  state.scenario = scenario;
  state.trusted = false;
  state.calls = {};
  state.workspaces = [];
  state.opened = null;
  state.reset += 1;
  window.location.hash = "#/settings?page=workspaces";
  publish();
}

function Fixture() {
  const [, redraw] = useState(0);
  const [language, setLanguage] = useState("zh");
  useEffect(() => {
    const listener = () => redraw((value) => value + 1);
    listeners.add(listener);
    return () => { listeners.delete(listener); };
  }, []);
  return (
    <main className="mx-auto flex min-h-screen w-full max-w-[800px] flex-col gap-5 px-4 py-6 text-text-primary sm:px-8">
      <header className="flex flex-col gap-3 rounded-2xl border border-separator-border bg-background-primary-default p-4">
        <h1 className="text-title-3-semibold">SSH 首次配置 · 浏览器验证</h1>
        <p role="note" className="text-body-medium text-text-error-primary">纯前端模拟：无真实 SSH、无模型、无外部网络请求；不读私钥或保存配置。</p>
        <p className="text-body-2-regular text-text-secondary">输入示例：主机 server.example，用户 demo。文件选择模拟返回示例路径；页面上的指纹不能用于真实服务器。</p>
        <div className="flex flex-wrap items-center gap-3 text-body-2-regular">
          <label className="flex items-center gap-2">模拟场景
            <select aria-label="模拟场景" className="rounded-md border border-separator-border bg-background-primary-default px-2 py-1" value={state.scenario} onChange={(event) => reset(event.target.value as Scenario)}>
              <option value="ready">首次信任 → 环境已就绪</option>
              <option value="changed">主机指纹冲突</option>
              <option value="missingPython">缺少 Python 3</option>
              <option value="unauthenticated">Claude 未登录</option>
            </select>
          </label>
          <label className="flex items-center gap-2">语言
            <select aria-label="界面语言" className="rounded-md border border-separator-border bg-background-primary-default px-2 py-1" value={language} onChange={(event) => { setLanguage(event.target.value); void i18n.changeLanguage(event.target.value); }}>
              <option value="zh">中文</option><option value="en">English</option>
            </select>
          </label>
          <button type="button" className="rounded-md border border-separator-border px-3 py-1" onClick={() => reset()}>重置空白表单</button>
        </div>
      </header>
      <SshWorkspacesCard key={state.reset} workspaces={state.workspaces} onRegistered={async (registered) => {
        state.workspaces = [...state.workspaces.filter((item) => item.id !== registered.id), registered];
        publish();
      }} />
      <section className="rounded-lg border border-separator-border p-3" aria-label="模拟调用记录">
        <h2 className="text-body-2-medium">仅本页内存状态</h2>
        <pre id="ssh-fixture-state" className="mt-2 whitespace-pre-wrap break-all text-caption-1-regular text-text-secondary">{JSON.stringify({ scenario: state.scenario, trusted: state.trusted, registeredProjects: state.workspaces.length, openedProject: state.opened, calls: state.calls }, null, 2)}</pre>
      </section>
    </main>
  );
}

window.location.hash = "#/settings?page=workspaces";
createRoot(document.getElementById("fixture")!).render(<Fixture />);
