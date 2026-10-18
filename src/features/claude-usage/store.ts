import { create } from "zustand";
import { ipc, type ClaudeAccountUsage, type ClaudeUsageWindow } from "@/lib/ipc";

interface UsageEntry {
  data: ClaudeAccountUsage | null;
  loading: boolean;
  error: string | null;
  attemptedAt: number;
}

interface UsageStore {
  accounts: Record<string, UsageEntry>;
  refresh: (account: string, workspacePath: string, force?: boolean) => Promise<void>;
}

const pending = new Map<string, Promise<void>>();
const CACHE_MS = 60_000;
const ERROR_BACKOFF_MS = 30_000;
const MAX_ACCOUNTS = 64;

/** Account-wide, memory-only data. No credentials are part of the response. */
export const useClaudeUsageStore = create<UsageStore>((set, get) => ({
  accounts: {},
  refresh: (account, workspacePath, force = false) => {
    const running = pending.get(account);
    if (running) return running;
    const previous = get().accounts[account];
    const age = Date.now() - (previous?.attemptedAt ?? 0);
    if (previous && ((previous.error && age < ERROR_BACKOFF_MS) || (!force && age < CACHE_MS))) return Promise.resolve();
    const accounts = { ...get().accounts };
    if (!previous && Object.keys(accounts).length >= MAX_ACCOUNTS) {
      const idle = Object.keys(accounts).filter((key) => !pending.has(key))
        .sort((left, right) => accounts[left].attemptedAt - accounts[right].attemptedAt)[0];
      if (!idle) return Promise.resolve();
      delete accounts[idle];
    }
    set(() => ({ accounts: { ...accounts, [account]: {
      data: previous?.data ?? null, loading: true, error: null, attemptedAt: Date.now(),
    } } }));
    const request = ipc.claudeAccountUsage(workspacePath, force)
      .then((data) => set((state) => ({ accounts: { ...state.accounts, [account]: {
        data, loading: false, error: null, attemptedAt: Date.now(),
      } } })))
      .catch((error: unknown) => set((state) => ({ accounts: { ...state.accounts, [account]: {
        ...state.accounts[account], loading: false, error: String(error), attemptedAt: Date.now(),
      } } })))
      .finally(() => { pending.delete(account); });
    pending.set(account, request);
    return request;
  },
}));

/** A reset passing makes the old percentage unknown, never assumed full. */
export function remainingPercent(window: ClaudeUsageWindow | null | undefined, now = Date.now()): number | null {
  if (!window || !Number.isFinite(window.usedPercent) || window.usedPercent < 0 || window.usedPercent > 100 ||
    (window.resetsAt !== null && window.resetsAt <= now)) return null;
  return Math.round((100 - window.usedPercent) * 10) / 10;
}
