import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { beforeEach, afterEach, expect, it, vi } from "vitest";
import { ipc, type Workspace } from "@/lib/ipc";
import i18n from "@/lib/i18n";
import { ClaudeQuotaButton } from "./ClaudeQuotaButton";
import { useClaudeUsageStore } from "./store";

vi.mock("@/lib/ipc", () => ({ ipc: { claudeAccountUsage: vi.fn() } }));
Object.assign(globalThis, { IS_REACT_ACT_ENVIRONMENT: true });
vi.stubGlobal("ResizeObserver", class { observe() {} unobserve() {} disconnect() {} });
const workspace: Workspace = { id: "remote", name: "remote", path: "/project", lastOpenedAt: null, sortOrder: 0, groupId: null,
  meta: { ssh: { host: "example.com", user: "cc", port: 22, identityFile: null, workspace: "/project", enginePaths: { claude: "claude" } } } };
let container: HTMLDivElement;
let root: Root;
beforeEach(() => { vi.resetAllMocks(); useClaudeUsageStore.setState({ accounts: {} }); container = document.createElement("div"); document.body.append(container); root = createRoot(container); });
afterEach(async () => { await act(async () => root.unmount()); container.remove(); });

it("shows remaining account percentages and reset times in the opened card", async () => {
  const reset = Date.now() + 60_000;
  vi.mocked(ipc.claudeAccountUsage).mockResolvedValue({ plan: "pro", fiveHour: { usedPercent: 100, resetsAt: reset }, sevenDay: { usedPercent: 41, resetsAt: reset + 10_000 }, updatedAt: Date.now() });
  await act(async () => root.render(<ClaudeQuotaButton workspacePath="/project" workspaces={[workspace]} streaming={false} />));
  expect(container.textContent).toContain("5h 0% · 周 59%");
  await act(async () => container.querySelector<HTMLButtonElement>("button")!.click());
  const bars = [...document.querySelectorAll('[role="progressbar"]')];
  expect(bars.map((bar) => bar.getAttribute("aria-valuenow"))).toEqual(["0", "59"]);
  expect(document.body.textContent).toContain(i18n.t("chat.quotaResets", { time: new Date(reset).toLocaleString() }));
  expect(ipc.claudeAccountUsage).toHaveBeenCalledOnce();
});

it("hides for local projects and shows unknown windows without inventing full allowance", async () => {
  await act(async () => root.render(<ClaudeQuotaButton workspacePath="/local" workspaces={[workspace]} streaming={false} />));
  expect(container.querySelector("button")).toBeNull();
  expect(ipc.claudeAccountUsage).not.toHaveBeenCalled();
  vi.mocked(ipc.claudeAccountUsage).mockResolvedValue({ plan: null, fiveHour: null, sevenDay: null, updatedAt: Date.now() });
  await act(async () => root.render(<ClaudeQuotaButton workspacePath="/project" workspaces={[workspace]} streaming={false} />));
  expect(container.textContent).toContain("5h — · 周 —");
  await act(async () => container.querySelector<HTMLButtonElement>("button")!.click());
  expect(document.querySelector('[role="progressbar"]')).toBeNull();
});

it("uses the child project's account rather than an ancestor and explains retry cooldown", async () => {
  const child = { ...workspace, id: "child", path: "/project/child", meta: { ssh: { host: "child.example.com", user: "other", port: 22, identityFile: null, workspace: "/project/child", enginePaths: { claude: "claude" } } } };
  vi.mocked(ipc.claudeAccountUsage).mockRejectedValue(new Error("Usage service unavailable"));
  await act(async () => root.render(<ClaudeQuotaButton workspacePath="/project/child" workspaces={[workspace, child]} streaming={false} />));
  expect(Object.keys(useClaudeUsageStore.getState().accounts)[0]).toContain("child.example.com");
  await act(async () => container.querySelector<HTMLButtonElement>("button")!.click());
  const retry = document.querySelector<HTMLButtonElement>(`button[aria-label="${i18n.t("chat.quotaRefresh")}"]`)!;
  expect(retry.disabled).toBe(true);
  expect(document.querySelector('[role="alert"]')?.textContent).toContain("重试");
  expect(document.querySelector('[role="alert"]')?.textContent).not.toContain(i18n.t("chat.quotaFailed"));
});
