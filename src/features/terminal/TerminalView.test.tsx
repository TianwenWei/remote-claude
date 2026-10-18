import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import type { ILink, ILinkProvider } from "@xterm/xterm";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { Workspace } from "@/lib/ipc";
import "@/lib/i18n";

const mocks = vi.hoisted(() => ({
  workspaces: [] as Workspace[],
  output: "",
  providers: [] as ILinkProvider[],
  terminalOpen: vi.fn(async () => {}),
  listDir: vi.fn(async () => [{ name: "artifact.txt" }]),
  reveal: vi.fn(async () => {}),
}));

vi.mock("@/lib/ipc", () => ({ ipc: {
  terminalOpen: mocks.terminalOpen,
  terminalWrite: vi.fn(async () => {}),
  terminalResize: vi.fn(async () => {}),
  listDir: mocks.listDir,
  revealInFileManager: mocks.reveal,
} }));
vi.mock("@/lib/events", () => ({ listenTerminalOutput: vi.fn(async () => () => {}) }));
vi.mock("@/features/chat/store", () => ({ useChatStore: { getState: () => ({ workspaces: mocks.workspaces }) } }));
vi.mock("./appearance", () => ({ terminalFontFamily: () => "monospace", terminalTheme: () => ({}) }));
vi.mock("./xterm-loader", () => ({ loadXterm: async () => ({
  Terminal: class {
    cols = 80;
    rows = 24;
    options = {};
    buffer = { active: { getLine: (index: number) => index === 0
      ? { isWrapped: false, translateToString: () => mocks.output } : undefined } };
    open() {}
    write() {}
    loadAddon() {}
    dispose() {}
    onData() { return { dispose() {} }; }
    registerLinkProvider(provider: ILinkProvider) { mocks.providers.push(provider); return { dispose() {} }; }
  },
  FitAddon: class { fit() {} },
  WebglAddon: class { onContextLoss() {} dispose() {} },
}) }));

import { TerminalView } from "./TerminalView";

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

describe("terminal reveal links", () => {
  let container: HTMLDivElement;
  let root: Root;

  beforeEach(() => {
    vi.clearAllMocks();
    mocks.providers.length = 0;
    mocks.workspaces = [];
    vi.stubGlobal("ResizeObserver", class { observe() {} disconnect() {} });
    container = document.createElement("div");
    document.body.appendChild(container);
    root = createRoot(container);
  });

  afterEach(async () => {
    await act(async () => root.unmount());
    container.remove();
    vi.unstubAllGlobals();
  });

  it("does not register the local reveal provider or probe SSH output paths", async () => {
    const cwd = "/home/cc/project";
    mocks.workspaces = [{ id: "ssh", name: "project", path: cwd, lastOpenedAt: null, sortOrder: null, groupId: null, meta: { ssh: {
      host: "vps", user: "cc", port: 22, identityFile: null, workspace: cwd, enginePaths: {},
    } } } satisfies Workspace];
    mocks.output = `${cwd}/artifact.txt`;
    await act(async () => root.render(<TerminalView id="remote-view" cwd={cwd} />));
    expect(mocks.terminalOpen).toHaveBeenCalledOnce();
    for (const provider of mocks.providers) provider.provideLinks(1, () => {});
    expect(mocks.providers).toHaveLength(0);
    expect(mocks.listDir).not.toHaveBeenCalled();
    expect(mocks.reveal).not.toHaveBeenCalled();
  });

  it("retains local path probing and modifier-click reveal", async () => {
    const cwd = "/tmp/local-terminal-link";
    mocks.output = `${cwd}/artifact.txt`;
    await act(async () => root.render(<TerminalView id="local-view" cwd={cwd} />));
    expect(mocks.providers).toHaveLength(1);
    const result = Promise.withResolvers<ILink[] | undefined>();
    mocks.providers[0].provideLinks(1, result.resolve);
    const links = await result.promise;
    expect(mocks.listDir).toHaveBeenCalledExactlyOnceWith(cwd);
    expect(links?.[0].text).toBe(`${cwd}/artifact.txt`);
    links?.[0].activate(new MouseEvent("click", { altKey: true, ctrlKey: true }), "");
    expect(mocks.reveal).toHaveBeenCalledExactlyOnceWith(`${cwd}/artifact.txt`);
  });
});
