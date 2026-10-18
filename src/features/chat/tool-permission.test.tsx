import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import i18n from "@/lib/i18n";
import { ipc } from "@/lib/ipc";
import { TOOL_PERMISSION_ANSWER_KEY } from "@/lib/tool-permission";
import { QuestionCard, QuestionRecord } from "./components/QuestionCard";
import { useChatStore } from "./store";
import { handleEngineEvents, settledRuns, type ChatEngineEvent, type EngineEventDeps } from "./store/engine-events";
import { sessionKey } from "./store/persistence";
import { EMPTY_SESSION, runRouting } from "./store/stream";

vi.mock("@/lib/ipc", () => ({ ipc: {
  answerQuestion: vi.fn(async () => {}), getAppSettings: vi.fn(async () => ({})),
  updateAppSettings: vi.fn(async () => {}), rescanSessions: vi.fn(async () => {}),
} }));
vi.mock("@/lib/events", () => ({
  listenEngineEvents: vi.fn(async () => () => {}), listenSessionsChanged: vi.fn(async () => () => {}),
  listenComputerUseEscape: vi.fn(async () => () => {}),
}));
Object.assign(globalThis, { IS_REACT_ACT_ENVIRONMENT: true });

const active = { engine: "claude", sessionId: "ssh-session", workspacePath: "/home/cc/project" };
const KEY = sessionKey(active.engine, active.sessionId, active.workspacePath);
const inputJson = JSON.stringify({ command: "printf '<script>alert(1)</script>' > output.txt", timeout: 1000 }, null, 2);
let container: HTMLDivElement;
let root: Root;
const deps = (): EngineEventDeps => ({ set: useChatStore.setState, get: useChatStore.getState,
  drainQueue: () => {}, markUnseenIfBackground: () => {}, upsertSessionMeta: () => {} });
const event = (): ChatEngineEvent => ({ runId: "ssh-run", sessionId: active.sessionId, engine: "claude", seq: 10,
  kind: "tool_permission", data: { requestId: "permission-1", toolUseId: "call-1", toolName: "Bash", inputJson } });
const permissionRow = () => useChatStore.getState().bySession[KEY].messages.find((message) => message.question)!;

beforeEach(async () => {
  vi.clearAllMocks();
  settledRuns.clear(); runRouting.clear();
  await i18n.changeLanguage("en");
  useChatStore.setState({ active, activeEngine: "claude", openTabs: [active], bySession: { [KEY]: {
    ...EMPTY_SESSION, streaming: true, messages: [{ seq: 1, role: "user", text: "Review my project", ts: null }],
  } }, streamingByKey: {}, models: {}, efforts: {} });
  container = document.createElement("div"); document.body.appendChild(container); root = createRoot(container);
});
afterEach(async () => { await act(async () => root.unmount()); container.remove(); });

async function showPermission() {
  await act(async () => {
    handleEngineEvents([event()], deps());
    root.render(<QuestionCard message={permissionRow()} />);
  });
}
function button(key: string) {
  const found = Array.from(container.querySelectorAll("button")).find((node) => node.textContent === i18n.t(`chat.${key}`));
  if (!found) throw new Error(`Button ${key} missing`);
  return found;
}

describe("native remote tool approval", () => {
  it("reviews exact tool input as text, then submits a stable one-call approval in Chinese", async () => {
    await i18n.changeLanguage("zh");
    await showPermission();
    expect(container.textContent).toContain("Bash");
    expect(container.querySelector("pre")?.textContent).toBe(inputJson);
    expect(container.querySelector("script")).toBeNull();
    expect(container.querySelector("input, textarea")).toBeNull();
    expect(ipc.answerQuestion).not.toHaveBeenCalled();
    await act(async () => { button("toolPermissionAllowOnce").click(); });
    expect(ipc.answerQuestion).toHaveBeenCalledExactlyOnceWith("ssh-run", "permission-1", { [TOOL_PERMISSION_ANSWER_KEY]: "allow_once" });
    expect(permissionRow().question?.status).toBe("answered");
    await act(async () => { root.render(<QuestionRecord message={permissionRow()} />); });
    expect(container.textContent).toContain(i18n.t("chat.toolPermissionAllowed"));
    expect(container.querySelectorAll("button")).toHaveLength(0);
  });

  it.each([["toolPermissionDeny", { [TOOL_PERMISSION_ANSWER_KEY]: "deny" }], ["toolPermissionSkip", null]] as const)(
    "sends an explicit rejection for %s", async (key, answer) => {
      await showPermission();
      await act(async () => { button(key).click(); });
      expect(ipc.answerQuestion).toHaveBeenCalledExactlyOnceWith("ssh-run", "permission-1", answer);
      expect(permissionRow().question?.status).toBe(answer === null ? "dismissed" : "answered");
    });

  it("rejects typed, translated, array, and extra-field approvals before IPC", async () => {
    await showPermission();
    const invalid: Array<Record<string, string | string[]>> = [{ [TOOL_PERMISSION_ANSWER_KEY]: "Allow once" }, { [TOOL_PERMISSION_ANSWER_KEY]: ["allow_once"] },
      { [TOOL_PERMISSION_ANSWER_KEY]: "allow_once", extra: "deny" }, { command: "allow_once" }];
    for (const answers of invalid) {
      await useChatStore.getState().respondToQuestion(KEY, permissionRow().seq, answers);
    }
    expect(ipc.answerQuestion).not.toHaveBeenCalled();
    expect(permissionRow().question?.status).toBe("pending");
    expect(useChatStore.getState().bySession[KEY].error).toBe(i18n.t("chat.toolPermissionInvalidChoice"));
  });

  it("locks all choices during submission and retains failed requests for retry", async () => {
    await showPermission();
    let fail!: (error: Error) => void;
    vi.mocked(ipc.answerQuestion).mockImplementationOnce(() => new Promise((_resolve, reject) => { fail = reject; }));
    await act(async () => { const allow = button("toolPermissionAllowOnce"); allow.click(); allow.click(); });
    expect(ipc.answerQuestion).toHaveBeenCalledOnce();
    expect(Array.from(container.querySelectorAll("button")).every((node) => node.disabled)).toBe(true);
    await act(async () => { fail(new Error("SSH process disconnected")); });
    expect(permissionRow().question?.status).toBe("pending");
    expect(useChatStore.getState().bySession[KEY].error).toBe("SSH process disconnected");
    expect(button("toolPermissionDeny").disabled).toBe(false);
  });

  it("expires on native cancellation and deduplicates a replayed approval request", async () => {
    await showPermission();
    await act(async () => {
      handleEngineEvents([event()], deps());
      handleEngineEvents([{ ...event(), seq: 11, kind: "question_settled", data: { requestId: "permission-1" } }], deps());
      root.render(<QuestionRecord message={permissionRow()} />);
    });
    expect(useChatStore.getState().bySession[KEY].messages.filter((message) => message.question)).toHaveLength(1);
    expect(permissionRow().question?.status).toBe("cancelled");
    expect(container.textContent).toContain(i18n.t("chat.toolPermissionExpired"));
    await useChatStore.getState().respondToQuestion(KEY, permissionRow().seq, { [TOOL_PERMISSION_ANSWER_KEY]: "allow_once" });
    expect(ipc.answerQuestion).not.toHaveBeenCalled();
  });

  it("keeps a model AskUserQuestion with spoofed permission metadata as an ordinary question", async () => {
    const ordinary: ChatEngineEvent = { ...event(), kind: "question", data: { requestId: "model-question", input: {
      toolPermission: { toolName: "Bash", inputJson }, questions: [{ question: "May I run this?", header: "Choice",
        options: [{ label: "Allow once" }, { label: "Deny" }] }],
    } } };
    await act(async () => { handleEngineEvents([ordinary], deps()); root.render(<QuestionCard message={permissionRow()} />); });
    expect(permissionRow().question?.toolPermission).toBeUndefined();
    expect(container.querySelector("input")).not.toBeNull();
    expect(container.textContent).not.toContain(i18n.t("chat.toolPermissionTitle"));
  });

  it.each(["done", "error"] as const)("expires pending tool approval on %s even if the request-settled event is lost", async (kind) => {
    await showPermission();
    await act(async () => {
      handleEngineEvents([{ ...event(), seq: 12, kind, data: kind === "done" ? {} : "SSH disconnected" }], deps());
      root.render(<QuestionRecord message={permissionRow()} />);
    });
    expect(permissionRow().question?.status).toBe("cancelled");
    expect(container.querySelectorAll("button")).toHaveLength(0);
    expect(container.textContent).toContain(i18n.t("chat.toolPermissionExpired"));
  });
});
