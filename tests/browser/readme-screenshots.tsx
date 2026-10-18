// README screenshots: production UI, exclusively synthetic and memory-only data.
// The running native app, private projects, credentials and models are untouched.
import { useRef } from "react";
import { createRoot } from "react-dom/client";
import { MemoryRouter } from "react-router-dom";
import "../../src/index.css";
import "../../src/lib/i18n";
import { AiChatSidebar, type AiChatRepo } from "../../src/components/application/ai-chat/ai-chat-sidebar";
import type { ComposerInputHandle } from "../../src/components/application/ai-chat/ai-chat-composer";
import { ChatConversation } from "../../src/features/chat/components/ChatConversation";
import { ChatPanelHeader } from "../../src/features/chat/ChatPanelHeader";
import { SessionTabStrip } from "../../src/features/chat/components/SessionTabStrip";
import { sessionKey, useChatStore } from "../../src/features/chat/store";
import { EMPTY_SESSION } from "../../src/features/chat/store/stream";
import { useGitStore } from "../../src/features/git/store";
import { useFilesStore } from "../../src/features/files/store";
import { useClaudeUsageStore } from "../../src/features/claude-usage/store";
import { TerminalDock } from "../../src/features/terminal/TerminalDock";
import { useTerminalStore } from "../../src/features/terminal/store";
import { SshWorkspacesCard } from "../../src/features/settings/SshWorkspacesCard";
import type { CliConfig, EngineInfo, Message, SshConnectionProbe, Workspace } from "../../src/lib/ipc";

const params = new URLSearchParams(window.location.search);
const view = params.get("view") === "setup" ? "setup" : params.get("view") === "terminal" ? "terminal" : "chat";
const path = "/home/demo/projects/task-board";
const active = { engine: "claude", sessionId: "readme-demo-chat", workspacePath: path };
const key = sessionKey(active.engine, active.sessionId, active.workspacePath);
const demoWorkspace: Workspace = {
  id: "demo-task-board", name: "task-board", path, lastOpenedAt: 0, sortOrder: 0, groupId: null,
  meta: { ssh: { host: "vps.example.com", user: "demo", port: 22, identityFile: null, workspace: path, enginePaths: { claude: "/demo/bin/claude" } } },
};
const workspaces = [demoWorkspace];
const engines: EngineInfo[] = [{ id: "claude", available: true, enabled: true, supportsImages: true, supportsEffort: true, supportsComputerUse: false, supportsMemory: false, permissions: ["manual", "auto", "plan"] }];
const now = new Date().toISOString();
const messages: Message[] = view === "terminal" ? [
  { seq: 1, role: "user", text: "运行测试，并检查当前 Git 状态。", ts: now },
  { seq: 2, role: "assistant", text: "12 项示例测试通过。你可以在下方终端继续操作这个远程项目。", ts: now, model: "opus", effort: "medium", durationMs: 18400,
    usage: { input_tokens: 24000, output_tokens: 1800, total_tokens: 25800, model_context_window: 200000 } },
] : [
  { seq: 1, role: "user", text: "为这个待办应用加上搜索和状态筛选，保持界面简洁。", ts: now },
  { seq: 3, role: "tool", text: "Read", path: `${path}/src/TaskList.tsx`, args: { file_path: `${path}/src/TaskList.tsx` }, result: "示例组件已读取。", ts: now },
  { seq: 4, role: "tool", text: "Edit", path: `${path}/src/TaskList.tsx`, args: { file_path: `${path}/src/TaskList.tsx`, old_string: "const visible = tasks;", new_string: "const visible = tasks.filter(matchesSearch).filter(matchesStatus);" }, result: "示例修改完成。", ts: now },
  { seq: 5, role: "tool", text: "Bash", args: { command: "npm test" }, result: "示例输出：12 tests passed", ts: now },
  { seq: 6, role: "assistant", text: "- **即时搜索**：按任务标题筛选。\n- **状态筛选**：全部、进行中、已完成，可与搜索组合使用。\n\n验证：`npm test`，12 项示例测试通过。", ts: now, model: "opus", effort: "medium", durationMs: 18400,
    usage: { input_tokens: 24000, output_tokens: 1800, total_tokens: 25800, model_context_window: 200000 } },
];
const section = { providers: {}, current: null };
const cliConfig = Object.fromEntries(["claude", "kimi", "grok", "codex", "pi", "omp", "dsh", "agy", "opencode", "qoder", "qoder-cn", "minimax"].map((engine) => [engine, section])) as unknown as CliConfig;
const fingerprint = "SHA256:DemoOnlyFingerprintVerifyOutsideThisFixture000";
let trusted = false;
const terminalOpened = new Set<string>();
const emit = (event: string, payload: unknown) => (window as unknown as { __readmeEmit: (event: string, payload: unknown) => void }).__readmeEmit(event, payload);
const infrastructureInvoke = window.__TAURI_INTERNALS__!.invoke!;
window.__TAURI_INTERNALS__!.invoke = async (command: string, rawArgs?: unknown) => {
  const args = (rawArgs ?? {}) as Record<string, unknown>;
  if (command.startsWith("plugin:event|")) return infrastructureInvoke(command, args);
  switch (command) {
    case "get_cli_config": return cliConfig;
    case "get_app_settings": return { theme: "light", language: "zh", customModels: {}, defaultModels: { claude: "opus" }, defaultEfforts: { claude: "medium" }, titlebar: "native" };
    case "list_engine_models": return { remote: true, authoritative: true, models: [{ id: "opus", name: "Opus", provider: "Anthropic", contextWindow: 200000 }, { id: "sonnet", name: "Sonnet", provider: "Anthropic", contextWindow: 200000 }] };
    case "terminal_open": {
      const id = String(args.id);
      if (!terminalOpened.has(id)) {
        terminalOpened.add(id);
        emit("terminal://output", [{ id, data: "\x1b[32mdemo@demo-vps\x1b[0m:~/projects/task-board$ npm test\r\n\r\n> task-board@1.0.0 test\r\n> vitest run\r\n\r\n \x1b[32m✓\x1b[0m src/TaskList.test.tsx (12 tests)\r\n\r\n Test Files  \x1b[32m1 passed\x1b[0m (1)\r\n      Tests  \x1b[32m12 passed\x1b[0m (12)\r\n\r\n\x1b[32mdemo@demo-vps\x1b[0m:~/projects/task-board$ " }]);
      }
      return null;
    }
    case "terminal_write": emit("terminal://output", [{ id: String(args.id), data: String(args.data).replace(/\r/g, "\r\n示例终端不会执行命令。\r\n$ ") }]); return null;
    case "terminal_resize":
    case "terminal_close": return null;
    case "inspect_ssh_host_key": return { status: trusted ? "known" : "new", keys: [{ algorithm: "ssh-ed25519", fingerprint }], knownFingerprints: trusted ? [fingerprint] : [] };
    case "trust_ssh_host_key":
      if (args.expectedFingerprint !== fingerprint) throw new Error("示例指纹不匹配。");
      trusted = true;
      return { status: "known", keys: [{ algorithm: "ssh-ed25519", fingerprint }], knownFingerprints: [fingerprint] };
    case "probe_ssh_connection": return {
      home: "/home/demo", canonicalPath: "/home/demo/projects", claudeAvailable: true, claudeVersion: "示例 Claude Code",
      directories: [{ name: "task-board", path }, { name: "docs-site", path: "/home/demo/projects/docs-site" }],
      dependencies: { bash: { available: true, version: "5" }, python: { available: true, version: "3" }, git: { available: true, version: "2" }, claude: { available: true, version: "示例" } },
      claudeAuth: { status: "authenticated", method: "claude.ai" },
    } satisfies SshConnectionProbe;
    case "plugin:dialog|open": return null;
    case "add_ssh_workspace":
    case "create_ssh_workspace": return demoWorkspace;
    default:
      // Every response is generated here. Unknown operations cannot reach an
      // app, filesystem, shell, socket, account or model.
      if (/^(send|write|delete|remove|exec|spawn|engine_send)/.test(command)) throw new Error("示例页面不会执行操作或调用模型。");
      return [];
  }
};

useChatStore.setState({
  active, activeEngine: "claude", workspaces, engines, initialized: true, openTabs: [{ ...active, model: "opus", effort: "medium" }],
  models: { claude: "opus" }, efforts: { claude: "medium" }, permission: "manual", sessions: [], providers: {},
  bySession: { [key]: { ...EMPTY_SESSION, messages, activeModel: "opus", activeEffort: "medium", usage: messages.at(-1)!.usage,
    historyCache: { cachedAt: Date.now(), refreshing: false, refreshError: null, updated: true } } },
  send: async () => {}, interrupt: async () => {}, startNewChat: () => {}, refreshSessions: async () => {},
});
useGitStore.setState({
  statusByWorkspace: { [path]: { branch: "main", staged: [], unstaged: [{ path: "src/TaskList.tsx", status: "M", additions: 18, deletions: 4 }], untracked: [] } },
  branchesByWorkspace: { [path]: [{ name: "main", isRemote: false }] }, refresh: async () => {}, loadBranches: async () => {},
});
useFilesStore.setState({ repositories: {}, selectedPath: null });
useClaudeUsageStore.setState({ refresh: async () => {} });
useTerminalStore.setState({ open: view === "terminal", height: 264, tabsByWorkspace: { [path]: [{ id: "readme-demo-terminal" }] }, activeByWorkspace: { [path]: "readme-demo-terminal" } });

const repos: AiChatRepo[] = [
  { id: demoWorkspace.id, path, label: "task-board", labelDescription: "SSH · vps.example.com", defaultOpen: true,
    threads: [{ id: key, label: "添加搜索与状态筛选", engine: "claude", time: "刚刚" }, { id: "demo-tests", label: "完善任务列表测试", engine: "claude", time: "2h" }, { id: "demo-layout", label: "优化移动端布局", engine: "claude", time: "昨天" }] },
  { id: "demo-docs", label: "docs-site", labelDescription: "SSH · vps.example.com", threads: [{ id: "demo-docs-chat", label: "更新快速开始指南", engine: "claude", time: "昨天" }] },
];

function Fixture() {
  const composerRef = useRef<ComposerInputHandle>(null);
  const headerRef = useRef<HTMLDivElement>(null);
  return <MemoryRouter><main className="flex h-dvh w-full min-w-[1000px] flex-col overflow-hidden bg-background-full text-text-primary">
    <div className="flex h-9 shrink-0 items-center justify-between border-b border-separator-border px-4 text-caption-1-medium text-text-secondary">
      <span>Remote Claude</span><span className="text-caption-2-regular text-text-tertiary">示例项目 · 模拟数据 · 无真实连接</span>
    </div>
    <div className="flex min-h-0 flex-1">
      <AiChatSidebar width={250} repos={repos} activeThreadId={key} onThreadSelect={() => {}} onNewRemoteProject={() => {}} onOpenSettings={() => {}} />
      <div className="flex min-h-0 min-w-0 flex-1 flex-col">
        {view === "setup" ? <div className="min-h-0 flex-1 overflow-auto px-8 py-6">
          <div className="mx-auto flex w-full max-w-[760px] flex-col gap-5"><h1 className="text-title-3-semibold">工作区</h1><SshWorkspacesCard workspaces={[]} onRegistered={async () => {}} /></div>
        </div> : <>
          <SessionTabStrip tabs={[{ key, label: "添加搜索与状态筛选", streaming: false, session: active }]} activeKey={key} onSelect={() => {}} onClose={() => {}} closeLabel="关闭" trafficLightInset={false}
            actions={<ChatPanelHeader workspacePath={path} activeTabKey={key} panelTab="files" onPanelTabChange={() => {}} panelCollapsed panelWidth={0} panelHeaderRef={headerRef} onTogglePanelCollapsed={() => {}} dragging={null} />} />
          <div className="flex min-h-0 flex-1 flex-col"><ChatConversation active={active} engines={engines} workspaces={workspaces} startNewChat={() => {}} composerInputRef={composerRef} /></div>
          <TerminalDock workspacePath={path} />
        </>}
      </div>
    </div>
  </main></MemoryRouter>;
}

createRoot(document.getElementById("fixture")!).render(<Fixture />);
