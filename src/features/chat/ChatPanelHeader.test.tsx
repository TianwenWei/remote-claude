import { act, type ReactNode } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { PluginInfo } from "@/lib/ipc";

const pluginReadArtwork = vi.fn(
  async (_id: string, _path: string): Promise<string> => "data:image/png;base64,AAAA",
);
vi.mock("@/lib/ipc", () => ({
  ipc: {
    pluginReadArtwork: (id: string, path: string) => pluginReadArtwork(id, path),
  },
}));
// Header only needs the plugins store; keep loader/builtin side effects and
// the panel implementations (which reach for IPC on mount) out of jsdom.
const STATES_SNAPSHOT: never[] = [];
vi.mock("@/features/plugins/runtime/loader", () => ({
  bootstrapPlugins: vi.fn(async () => []),
  getPluginStatesSnapshot: () => STATES_SNAPSHOT,
  ipcBackend: {},
  loadPlugin: vi.fn(async () => {}),
  reloadPlugin: vi.fn(async () => {}),
  pluginsBootstrapped: () => true,
  prunePluginRuntimeState: vi.fn(),
  subscribePluginStates: () => () => {},
  unloadPlugin: vi.fn(),
}));
vi.mock("@/features/plugins/builtin", () => ({ BUILTIN_PLUGINS: [] }));
vi.mock("@/features/open-app/HeaderOpenActions", () => ({ HeaderOpenActions: () => <button aria-label="终端" /> }));
vi.mock("@/features/launch-script/LaunchScriptActions", () => ({ LaunchScriptActions: () => null }));
vi.mock("@/features/files/FilesPanel", () => ({ FilesPanel: () => null }));
vi.mock("@/features/git/ChangesPanel", () => ({ ChangesPanel: () => null }));

import "@/lib/i18n";
import { panelTabRegistry } from "@ccgui/plugin-sdk";
import { usePluginsStore } from "@/features/plugins/manager/usePlugins";
import { ChatPanelHeader } from "./ChatPanelHeader";
import { sessionKey, useChatStore } from "./store";
import { EMPTY_SESSION } from "./store/stream";
// Side-effect import: registers the builtin files/changes tabs.
import "./panel-tabs";

// React 18's act() requires this flag to be set by the test environment.
declare global {
  // eslint-disable-next-line no-var
  var IS_REACT_ACT_ENVIRONMENT: boolean;
}
globalThis.IS_REACT_ACT_ENVIRONMENT = true;

// jsdom has no ResizeObserver; PillTabList measures its selection thumb with it.
class ResizeObserverStub {
  observe() {}
  unobserve() {}
  disconnect() {}
}
vi.stubGlobal("ResizeObserver", ResizeObserverStub);

function installedPlugin(overrides: Partial<PluginInfo> & { id: string }): PluginInfo {
  return {
    name: "Kimi LB",
    version: "1.0.0",
    description: "",
    author: "ccgui",
    tier: "js",
    source: "marketplace",
    enabled: true,
    quarantined: false,
    lastError: null,
    permissions: ["ui:panel-tab"],
    installedAt: 0,
    minAppVersion: null,
    icon: null,
    screenshots: [],
    ...overrides,
  };
}

describe("ChatPanelHeader", () => {
  let container: HTMLDivElement;
  let root: Root;
  let disposers: Array<() => void>;
  const onPanelTabChange = vi.fn();
  const selectSession = useChatStore.getState().selectSession;

  function header(panelTab = "files", activeTabKey: string | null = null): ReactNode {
    return (
      <ChatPanelHeader
        workspacePath="/tmp/ws"
        activeTabKey={activeTabKey}
        panelTab={panelTab}
        onPanelTabChange={onPanelTabChange}
        panelCollapsed={false}
        onTogglePanelCollapsed={() => {}}
        panelWidth={300}
        panelHeaderRef={{ current: null }}
        dragging={null}
      />
    );
  }

  beforeEach(() => {
    pluginReadArtwork.mockClear();
    onPanelTabChange.mockClear();
    usePluginsStore.setState({ installed: [] });
    useChatStore.setState({ active: null, workspaces: [], bySession: {}, selectSession });
    disposers = [];
    container = document.createElement("div");
    document.body.appendChild(container);
    root = createRoot(container);
  });

  afterEach(() => {
    act(() => {
      for (const dispose of disposers) dispose();
      root.unmount();
    });
    container.remove();
    useChatStore.setState({ selectSession });
  });

  /** Render and flush the artwork read's promise chain. */
  async function render(node: ReactNode = header()) {
    await act(async () => {
      root.render(node);
    });
  }

  function pill(label: string): HTMLButtonElement | null {
    return container.querySelector<HTMLButtonElement>(`button[aria-label="${label}"]`);
  }

  function pillByText(text: string): HTMLButtonElement | null {
    return (
      Array.from(container.querySelectorAll<HTMLButtonElement>("button")).find(
        (button) => button.textContent === text,
      ) ?? null
    );
  }

  it("places remote history refresh beside Terminal and targets the active conversation", async () => {
    const active = { engine: "claude", sessionId: "remote-session", workspacePath: "/tmp/ws" };
    const key = sessionKey(active.engine, active.sessionId, active.workspacePath);
    const refresh = vi.fn().mockResolvedValue(undefined);
    useChatStore.setState({
      active,
      selectSession: refresh,
      workspaces: [{ id: "remote", name: "ws", path: "/tmp/ws", lastOpenedAt: null, sortOrder: 0, groupId: null,
        meta: { ssh: { host: "example.com", user: "cc", port: 22, identityFile: null, workspace: "/tmp/ws", enginePaths: { claude: "/usr/bin/claude" } } } }],
      bySession: { [key]: { ...EMPTY_SESSION, historyCache: { cachedAt: 1000, refreshing: false, refreshError: null } } },
    });
    await render(header("files", key));
    const buttons = [...container.querySelectorAll("button")];
    expect(buttons[buttons.findIndex((button) => button.ariaLabel === "终端") + 1]?.ariaLabel).toBe("更新远程记录");
    await act(async () => pill("更新远程记录")!.click());
    expect(refresh).toHaveBeenCalledWith("claude", "remote-session", "/tmp/ws");
    // A file/other conversation tab must not refresh the hidden chat.
    await render(header("files", "file:/tmp/ws/readme.md"));
    expect(pill("更新远程记录")).toBeNull();
    await act(async () => useChatStore.setState({ workspaces: [] }));
    await render(header("files", key));
    expect(pill("更新远程记录")).toBeNull();
  });

  it("keeps icon + label for builtin tabs", () => {
    act(() => {
      root.render(header());
    });
    const files = pillByText("文件");
    expect(files?.querySelector("svg")).not.toBeNull();
    expect(pillByText("变更")?.textContent).toBe("变更");
  });

  it("renders a plugin pill icon-only with the label as title/accessible name", () => {
    disposers.push(
      panelTabRegistry.register({
        id: "plugin:kimi-lb",
        label: () => "Kimi LB",
        icon: () => <svg data-testid="registered-tab-icon" />,
        component: () => null,
      }),
    );
    act(() => {
      root.render(header());
    });
    const kimi = pill("Kimi LB");
    expect(kimi).not.toBeNull();
    expect(kimi?.getAttribute("title")).toBe("Kimi LB");
    expect(kimi?.querySelector('[data-testid="registered-tab-icon"]')).not.toBeNull();
    // The plugin-supplied label never renders as text.
    expect(kimi?.textContent).toBe("");
    act(() => {
      kimi?.dispatchEvent(new MouseEvent("click", { bubbles: true }));
    });
    expect(onPanelTabChange).toHaveBeenCalledWith("plugin:kimi-lb");
  });

  it("falls back to the plugin's manifest artwork when no icon is registered", async () => {
    usePluginsStore.setState({
      installed: [installedPlugin({ id: "kimi-lb", icon: "docs/icon.png" })],
    });
    disposers.push(
      panelTabRegistry.register({
        id: "plugin:kimi-lb",
        label: () => "Kimi LB",
        component: () => null,
      }),
    );
    await render();
    expect(pluginReadArtwork).toHaveBeenCalledWith("kimi-lb", "docs/icon.png");
    const img = pill("Kimi LB")?.querySelector("img");
    expect(img?.getAttribute("src")).toBe("data:image/png;base64,AAAA");
    // The pill label never renders; only the tile initial sits under the img.
    expect(pill("Kimi LB")?.textContent).toBe("K");
  });

  it("keeps the artwork read across header re-renders (stable glyph identity)", async () => {
    usePluginsStore.setState({
      installed: [installedPlugin({ id: "kimi-lb", icon: "docs/icon.png" })],
    });
    disposers.push(
      panelTabRegistry.register({
        id: "plugin:kimi-lb",
        label: () => "Kimi LB",
        component: () => null,
      }),
    );
    await render();
    expect(pluginReadArtwork).toHaveBeenCalledTimes(1);
    await render(header("changes"));
    expect(pluginReadArtwork).toHaveBeenCalledTimes(1);
  });

  it("falls back to the deterministic letter tile without artwork", async () => {
    disposers.push(
      panelTabRegistry.register({
        id: "plugin:code-check",
        label: () => "代码体检",
        component: () => null,
      }),
    );
    await render();
    const check = pill("代码体检");
    expect(check?.textContent).toBe("代");
    const tile = Array.from(check?.querySelectorAll("span") ?? []).find((span) =>
      span.style.backgroundImage.includes("linear-gradient"),
    );
    expect(tile?.textContent).toBe("代");
  });
});
