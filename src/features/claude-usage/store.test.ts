import { beforeEach, afterEach, describe, expect, it, vi } from "vitest";
import { ipc, type ClaudeAccountUsage } from "@/lib/ipc";
import { remainingPercent, useClaudeUsageStore } from "./store";

vi.mock("@/lib/ipc", () => ({ ipc: { claudeAccountUsage: vi.fn() } }));
const snapshot: ClaudeAccountUsage = { plan: "pro", fiveHour: { usedPercent: 100, resetsAt: 4_000_000 }, sevenDay: { usedPercent: 41, resetsAt: 5_000_000 }, updatedAt: 1_000_000 };

describe("Claude account quota", () => {
  beforeEach(() => { vi.useFakeTimers(); vi.setSystemTime(1_000_000); vi.resetAllMocks(); useClaudeUsageStore.setState({ accounts: {} }); });
  afterEach(() => { vi.useRealTimers(); });

  it("coalesces simultaneous refreshes of the same account across project paths", async () => {
    let resolve!: (data: ClaudeAccountUsage) => void;
    vi.mocked(ipc.claudeAccountUsage).mockReturnValueOnce(new Promise((done) => { resolve = done; }));
    const first = useClaudeUsageStore.getState().refresh("account-a", "/project-a");
    const second = useClaudeUsageStore.getState().refresh("account-a", "/project-b", true);
    expect(second).toBe(first);
    expect(ipc.claudeAccountUsage).toHaveBeenCalledOnce();
    resolve(snapshot);
    await first;
    expect(useClaudeUsageStore.getState().accounts["account-a"].data).toEqual(snapshot);
    await useClaudeUsageStore.getState().refresh("account-a", "/project-b");
    expect(ipc.claudeAccountUsage).toHaveBeenCalledOnce();
  });

  it("retains last good values on error and backs off repeated clicks", async () => {
    vi.mocked(ipc.claudeAccountUsage).mockResolvedValueOnce(snapshot).mockRejectedValueOnce(new Error("Usage request unavailable"));
    await useClaudeUsageStore.getState().refresh("account-a", "/project-a");
    await useClaudeUsageStore.getState().refresh("account-a", "/project-a", true);
    expect(useClaudeUsageStore.getState().accounts["account-a"]).toMatchObject({ data: snapshot, loading: false, error: "Error: Usage request unavailable" });
    await useClaudeUsageStore.getState().refresh("account-a", "/project-a", true);
    expect(ipc.claudeAccountUsage).toHaveBeenCalledTimes(2);
    vi.advanceTimersByTime(31_000);
    vi.mocked(ipc.claudeAccountUsage).mockResolvedValueOnce(snapshot);
    await useClaudeUsageStore.getState().refresh("account-a", "/project-a", true);
    expect(useClaudeUsageStore.getState().accounts["account-a"].error).toBeNull();
  });

  it("isolates accounts and does not fabricate missing or expired percentages", async () => {
    vi.mocked(ipc.claudeAccountUsage).mockResolvedValueOnce(snapshot).mockResolvedValueOnce({ ...snapshot, fiveHour: null });
    await Promise.all([useClaudeUsageStore.getState().refresh("a", "/a"), useClaudeUsageStore.getState().refresh("b", "/b")]);
    expect(useClaudeUsageStore.getState().accounts.a.data?.fiveHour).toEqual(snapshot.fiveHour);
    expect(useClaudeUsageStore.getState().accounts.b.data?.fiveHour).toBeNull();
    expect(remainingPercent(snapshot.fiveHour)).toBe(0);
    expect(remainingPercent(snapshot.sevenDay)).toBe(59);
    expect(remainingPercent(null)).toBeNull();
    expect(remainingPercent({ usedPercent: 10, resetsAt: 999_999 })).toBeNull();
    expect(remainingPercent({ usedPercent: Number.NaN, resetsAt: null })).toBeNull();
  });

  it("bounds inactive account snapshots across changing SSH identities", async () => {
    vi.mocked(ipc.claudeAccountUsage).mockResolvedValue(snapshot);
    for (let index = 0; index < 65; index++) {
      await useClaudeUsageStore.getState().refresh(`account-${index}`, "/project");
      vi.advanceTimersByTime(1);
    }
    expect(Object.keys(useClaudeUsageStore.getState().accounts)).toHaveLength(64);
    expect(useClaudeUsageStore.getState().accounts["account-0"]).toBeUndefined();
    expect(useClaudeUsageStore.getState().accounts["account-64"].data).toEqual(snapshot);
  });
});
