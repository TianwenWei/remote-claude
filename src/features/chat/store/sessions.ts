import { normalizeOmpServiceTier } from "@/lib/omp-service-tier";
import {
  ipc,
  type EngineInfo,
  type Message,
  type SessionMeta,
  type Workspace,
} from "@/lib/ipc";
import type { EffortLevel } from "@/components/application/ai-chat/cli-menu";
import { listenEngineEvents, listenSessionsChanged, listenComputerUseEscape } from "@/lib/events";
import { errorText } from "@/lib/errors";
import { writeStored } from "@/lib/storage";
import { subscribeTauriEvent } from "@/hooks/use-tauri-event";
import {
  CLI_CONFIG_CHANGED_EVENT,
  engineCurrents,
} from "@/features/settings/providers";
import {
  ENGINE_PREF_KEY,
  persistTabs,
  readPersistedActive,
  readPersistedTabs,
  sameTab,
  sessionKey,
  type ActiveSession,
} from "./persistence";
import { EMPTY_SESSION, patchSession } from "./stream";
import { handleEngineEvents, upsertSessionMetaInto } from "./engine-events";
import { mergePlanReviewHistory } from "./plan-review";
import i18n from "@/lib/i18n";
import {
  listExternalSessionMetas,
  setSessionSourcesChangedCallback,
} from "@/features/plugins/runtime/session-source";
import { emitSessionActivated } from "@/features/plugins/runtime/events";
import {
  dispatchSessionClosed,
  dispatchSessionRestored,
} from "@/features/plugins/runtime/hooks";
import { clearScopedContributions, sessionLifecycleBase } from "./lifecycle";
import {
  mergeExternalSessions,
  preserveUnscannedSessions,
  visibleSessions,
} from "./session-utils";
import type { ChatStore } from "./types";
import type { LoadCachedHistoryPage, LoadHistoryPage, StoreGet, StoreSet } from "./context";
import { listSshSessionMetas } from "./ssh-sessions";
import { cachedSshSessionMetas, removeCachedSshSessionMeta } from "./ssh-session-catalog";

function omitKey(rec: Record<string, boolean>, key: string) {
  const next = { ...rec };
  delete next[key];
  return next;
}

/** A refreshed tail replaces the cached window while older pages already
 * opened by the reader stay visible. A live turn owns its current rows. */
function refreshCachedMessages(current: Message[], fresh: Message[]): Message[] {
  if (!fresh.length) return fresh;
  const first = Math.min(...fresh.map((message) => message.seq));
  return [...current.filter((message) => message.seq < first), ...fresh];
}

function archivedKeyMap(sessions: SessionMeta[]): Record<string, true> {
  return Object.fromEntries(
    sessions.map((session) => [
      sessionKey(session.engine, session.sessionId, session.workspacePath),
      true as const,
    ]),
  );
}

function withoutArchived(
  sessions: SessionMeta[],
  archived: Record<string, true>,
): SessionMeta[] {
  return sessions.filter(
    (session) =>
      !archived[sessionKey(session.engine, session.sessionId, session.workspacePath)],
  );
}

/**
 * Session lifecycle: init/refresh of the session/engine catalogs, history
 * paging, and sidebar session ops (archive/delete/pin/rename).
 */

export interface SessionDeps {
  set: StoreSet;
  get: StoreGet;
  loadHistoryPage: LoadHistoryPage;
  loadCachedHistoryPage: LoadCachedHistoryPage;
  /** Module-scope unlisteners (store.ts) init pushes into; HMR dispose runs them. */
  eventTeardowns: Array<() => void>;
  activateTab: (tab: ActiveSession | null) => void;
  removeTab: (
    engine: string,
    sessionId: string | null,
    workspacePath: string,
  ) => void;
  forgetClosedTab: (key: string) => void;
  drainQueue: (key: string) => void;
  markUnseenIfBackground: (key: string) => void;
  /** 一轮结束后的复盘计数钩子（store.ts 接记忆模块）。 */
  turnSettled: (key: string) => void;
}

export function createSessionActions(
  deps: SessionDeps,
): Pick<
  ChatStore,
  | "init"
  | "refreshSessions"
  | "refreshEngines"
  | "selectSession"
  | "loadEarlier"
  | "archiveSession"
  | "deleteSession"
  | "pinSession"
  | "renameSession"
> {
  const {
    set,
    get,
    loadHistoryPage,
    loadCachedHistoryPage,
    eventTeardowns,
    activateTab,
    removeTab,
    forgetClosedTab,
    drainQueue,
    markUnseenIfBackground,
    turnSettled,
  } = deps;

  let sshCatalogGeneration = 0;
  let sshHistoryActionError: string | null = null;

  function sshHistoryErrorPatch(errors: Array<{ name: string; error: string }>, current: string | null): { actionError?: string | null } {
    if (errors.length) {
      // A catalog failure must not replace an unrelated action's diagnosis.
      if (current && current !== sshHistoryActionError) return {};
      sshHistoryActionError = errors.map((error) => i18n.t("settings.sshHistoryFailed", error)).join("\n");
      return { actionError: sshHistoryActionError };
    }
    const owned = sshHistoryActionError !== null && current === sshHistoryActionError;
    sshHistoryActionError = null;
    return owned ? { actionError: null } : {};
  }

  /** Migrate the engine pref off a CLI that is gone or disabled in
   * settings. All CLIs disabled: leave the pref alone — the composer shows
   * the "no CLI enabled" placeholder instead of a misleading fallback. */
  function ensureUsableEngine(engines: EngineInfo[]) {
    const usable = engines.filter((e) => e.enabled);
    if (usable.length === 0 || usable.some((e) => e.id === get().activeEngine))
      return;
    get().setActiveEngine(usable.find((e) => e.available)?.id ?? usable[0].id);
  }

  return {
    init: async () => {
      if (get().initialized) return;
      set({ initialized: true });
      eventTeardowns.push(
        subscribeTauriEvent(() =>
          listenEngineEvents((events) =>
            handleEngineEvents(events, {
              set,
              get,
              drainQueue,
              markUnseenIfBackground,
              upsertSessionMeta: (meta) => upsertSessionMetaInto(set, meta),
              refreshSessionUsage: (k) => get().refreshSessionUsage(k),
              turnSettled,
            }),
          ),
        ),
        subscribeTauriEvent(() =>
          listenSessionsChanged(() => void get().refreshSessions()),
        ),
        // Global Esc during a computer-use run: the backend arms that hotkey
        // only while a run is active, so this fires exactly when the user
        // needs the escape hatch out of a machine-driving turn.
        subscribeTauriEvent(() =>
          listenComputerUseEscape(() => void get().interrupt()),
        ),
      );
      // Settings' CLI enable switch / channel edits: re-filter history and
      // picker options without a restart.
      const onCliConfigChanged = () => void get().refreshEngines();
      window.addEventListener(CLI_CONFIG_CHANGED_EVENT, onCliConfigChanged);
      eventTeardowns.push(() =>
        window.removeEventListener(CLI_CONFIG_CHANGED_EVENT, onCliConfigChanged),
      );
      const [workspaces, sessions, archivedSessions, engines] = await Promise.all([
        ipc.listWorkspaces().catch(() => [] as Workspace[]),
        ipc.listSessions().catch(() => [] as SessionMeta[]),
        ipc.listArchivedSessions().catch(() => [] as SessionMeta[]),
        ipc.listEngines().catch(() => [] as EngineInfo[]),
      ]);
      // Plugin session sources (remote/容器内 CLI) merge under the local
      // scan so WSL 等远端会话进侧栏,且重启时已开标签不至于因“会话表无
      // 此会话”被清掉。
      setSessionSourcesChangedCallback(() => void get().refreshSessions());
      const archivedSessionKeys = archivedKeyMap(archivedSessions);
      const cached = cachedSshSessionMetas(workspaces);
      // Publish the offline catalog before any remote listing can block.
      set({ workspaces, engines, archivedSessionKeys, sessions: visibleSessions(withoutArchived(
        mergeExternalSessions(sessions, cached, workspaces.map((workspace) => workspace.path)), archivedSessionKeys,
      ), engines) });
      const external = await listExternalSessionMetas();
      const allSessions = withoutArchived(
        mergeExternalSessions(sessions, [...external, ...cached], workspaces.map((w) => w.path)),
        archivedSessionKeys,
      );
      set({
        workspaces,
        sessions: visibleSessions(allSessions, engines),
        archivedSessionKeys,
        engines,
      });
      ensureUsableEngine(engines);
      // Restore persisted tabs; drop ones whose workspace/session is gone.
      const restoredTabs = readPersistedTabs().filter(
        (t) =>
          workspaces.some((w) => w.path === t.workspacePath) &&
          (t.sessionId === null ||
            allSessions.some(
              (s) => s.engine === t.engine && s.sessionId === t.sessionId,
            )),
      );
      set({ openTabs: restoredTabs });
      const persistedActive = readPersistedActive();
      const activeTab =
        persistedActive &&
        restoredTabs.some((t) =>
          sameTab(
            t,
            persistedActive.engine,
            persistedActive.sessionId,
            persistedActive.workspacePath,
          ),
        )
          ? persistedActive
          : (restoredTabs[0] ?? null);
      if (activeTab) activateTab(activeTab);
      ipc
        .getAppSettings()
        .then((settings) =>
          set({
            efforts: (settings.defaultEfforts ?? {}) as Record<
              string,
              EffortLevel
            >,
            ompServiceTier: normalizeOmpServiceTier(
              settings.ompOpenaiServiceTier,
            ),
            codexServiceTier: normalizeOmpServiceTier(
              settings.codexServiceTier,
            ),
            models: settings.defaultModels ?? {},
            threadLimit: settings.sidebarThreadLimit ?? 5,
            workspaceGroups: settings.workspaceGroups ?? [],
            workspaceAliases: settings.workspaceAliases ?? {},
            archivedWorkspaces: settings.archivedWorkspaces ?? [],
            sendShortcut: settings.composerSendShortcut ?? "enter",
            thinkingAutoCollapse: settings.thinkingAutoCollapse ?? true,
            wideLayout: settings.chatWideLayout ?? false,
          }),
        )
        .catch(() => {});
      ipc
        .getCliConfig?.()
        ?.then((config) => set({ providers: engineCurrents(config) }))
        .catch(() => {});
      // Catalog refresh is background work: local snapshots and restored
      // tabs remain usable during a slow or unavailable SSH connection.
      const catalogGeneration = ++sshCatalogGeneration;
      void listSshSessionMetas(workspaces, get().sessions, get().archivedSessionKeys).then((ssh) => {
        if (catalogGeneration !== sshCatalogGeneration) return;
        for (const meta of ssh.sessions) {
          if (get().archivedSessionKeys[sessionKey(meta.engine, meta.sessionId, meta.workspacePath)]) {
            removeCachedSshSessionMeta(workspaces, meta.engine, meta.sessionId, meta.workspacePath);
          }
        }
        set((state) => ({
          sessions: visibleSessions(withoutArchived(preserveUnscannedSessions(
            mergeExternalSessions(sessions, [...external, ...ssh.sessions], workspaces.map((workspace) => workspace.path)),
            state.sessions, state.bySession,
          ), state.archivedSessionKeys), state.engines),
          ...sshHistoryErrorPatch(ssh.errors, state.actionError),
        }));
      });
    },

    refreshSessions: async () => {
      const catalogGeneration = ++sshCatalogGeneration;
      const [sessions, external, archivedSessions, ssh] = await Promise.all([
        ipc.listSessions().catch(() => null),
        listExternalSessionMetas(),
        ipc.listArchivedSessions().catch(() => null),
        listSshSessionMetas(get().workspaces, get().sessions, get().archivedSessionKeys),
      ]);
      if (!sessions || !archivedSessions || catalogGeneration !== sshCatalogGeneration) return;
      const archivedSessionKeys = archivedKeyMap(archivedSessions);
      const merged = withoutArchived(
        mergeExternalSessions(sessions, [...external, ...ssh.sessions], get().workspaces.map((w) => w.path)),
        archivedSessionKeys,
      );
      for (const session of archivedSessions) {
        removeCachedSshSessionMeta(get().workspaces, session.engine, session.sessionId, session.workspacePath);
        clearScopedContributions(set, session.engine, session.sessionId, session.workspacePath);
        const tab = get().openTabs.find((candidate) =>
          sameTab(candidate, session.engine, session.sessionId, session.workspacePath),
        );
        if (tab) dispatchSessionClosed(sessionLifecycleBase(get, tab));
      }
      set((s) => {
        const visible = visibleSessions(
          withoutArchived(
            preserveUnscannedSessions(merged, s.sessions, s.bySession),
            archivedSessionKeys,
          ),
          s.engines,
        );
        const bySession = { ...s.bySession };
        const drafts = { ...s.drafts };
        const unseen = { ...s.unseen };
        const streamingByKey = { ...s.streamingByKey };
        const retryingByKey = { ...s.retryingByKey };
        for (const key of Object.keys(archivedSessionKeys)) {
          delete bySession[key];
          delete drafts[key];
          delete unseen[key];
          delete streamingByKey[key];
          delete retryingByKey[key];
        }
        for (const meta of merged) {
          const key = sessionKey(meta.engine, meta.sessionId, meta.workspacePath);
          const current = bySession[key];
          if (!current) continue;
          bySession[key] = {
            ...current,
            // Saved selectors drive the next send, not the model already running.
            activeModel: current.streaming && current.activeModel
              ? current.activeModel
              : meta.model ?? current.activeModel,
            activeEffort: meta.effort ?? current.activeEffort,
            activeProvider: meta.provider ?? current.activeProvider,
          };
        }
        const openTabs = s.openTabs.filter(
          (tab) =>
            tab.sessionId === null ||
            !archivedSessionKeys[sessionKey(tab.engine, tab.sessionId, tab.workspacePath)],
        );
        const active =
          s.active?.sessionId &&
          archivedSessionKeys[
            sessionKey(s.active.engine, s.active.sessionId, s.active.workspacePath)
          ]
            ? (openTabs[0] ?? null)
            : s.active;
        persistTabs(openTabs, active);
        return {
          sessions: visible,
          ...sshHistoryErrorPatch(ssh.errors, s.actionError),
          archivedSessionKeys,
          bySession,
          drafts,
          unseen,
          streamingByKey,
          retryingByKey,
          openTabs,
          active,
        };
      });
    },

    refreshEngines: async () => {
      const catalogGeneration = ++sshCatalogGeneration;
      const [engines, sessions, external, archivedSessions, config, ssh] = await Promise.all([
        ipc.listEngines().catch(() => null),
        ipc.listSessions().catch(() => null),
        listExternalSessionMetas(),
        ipc.listArchivedSessions().catch(() => null),
        ipc.getCliConfig?.().catch(() => null) ?? Promise.resolve(null),
        listSshSessionMetas(get().workspaces, get().sessions, get().archivedSessionKeys),
      ]);
      if (!engines) return;
      if (catalogGeneration !== sshCatalogGeneration) {
        // Keep engine settings responsive while a newer catalog scan owns
        // sidebar rows and its error state.
        set({ engines, ...(config ? { providers: engineCurrents(config) } : {}) });
        ensureUsableEngine(engines);
        return;
      }
      for (const session of archivedSessions ?? []) removeCachedSshSessionMeta(get().workspaces, session.engine, session.sessionId, session.workspacePath);
      set((s) => {
        const keys = archivedSessions
          ? archivedKeyMap(archivedSessions)
          : s.archivedSessionKeys;
        return {
          engines,
          ...sshHistoryErrorPatch(ssh.errors, s.actionError),
          archivedSessionKeys: keys,
          ...(sessions
            ? {
                sessions: visibleSessions(
                  withoutArchived(
                    preserveUnscannedSessions(
                      mergeExternalSessions(
                        sessions,
                        [...external, ...ssh.sessions],
                        s.workspaces.map((w) => w.path),
                      ),
                      s.sessions,
                      s.bySession,
                    ),
                    keys,
                  ),
                  engines,
                ),
              }
            : {}),
          ...(config ? { providers: engineCurrents(config) } : {}),
        };
      });
      ensureUsableEngine(engines);
    },

    selectSession: async (engine, sessionId, workspacePath) => {
      const key = sessionKey(engine, sessionId, workspacePath);
      // The session remembers the model it ran, and our own record is the
      // only place that carries the provider ("agentrouter qunyou/x", while
      // the engine transcript keeps the bare "x"). Without it a session
      // reopened here — after a restart, or in another window — showed the
      // engine default and sent that instead.
      const remembered = get().sessions.find(
        (x) => x.engine === engine && x.sessionId === sessionId,
      )?.model;
      if (remembered && !get().bySession[key]?.activeModel) {
        patchSession(set, key, { activeModel: remembered });
      }
      // Same for the reasoning level: the picker and the next send follow the
      // session, so an unset level adopts the one this session last ran.
      const rememberedEffort = get().sessions.find(
        (x) => x.engine === engine && x.sessionId === sessionId,
      )?.effort;
      if (rememberedEffort && !get().bySession[key]?.activeEffort) {
        patchSession(set, key, { activeEffort: rememberedEffort });
      }
      const rememberedProvider = get().sessions.find(
        (x) => x.engine === engine && x.sessionId === sessionId,
      )?.provider;
      if (rememberedProvider && !get().bySession[key]?.activeProvider) {
        patchSession(set, key, { activeProvider: rememberedProvider });
      }
      const syncEngine = engine !== get().activeEngine;
      if (syncEngine) writeStored(ENGINE_PREF_KEY, engine);
      set((s) => {
        // Native sessions read effort from SessionState/database. Strip the
        // legacy per-tab field so an old localStorage value cannot shadow it.
        const stored = s.openTabs.find((t) =>
          sameTab(t, engine, sessionId, workspacePath),
        );
        const tab: ActiveSession = stored
          ? { ...stored, effort: undefined, provider: undefined }
          : { engine, sessionId, workspacePath };
        const openTabs = stored
          ? s.openTabs.map((t) => (t === stored ? tab : t))
          : [...s.openTabs, tab];
        persistTabs(openTabs, tab);
        const unseen = key in s.unseen ? omitKey(s.unseen, key) : s.unseen;
        return { openTabs, active: tab, unseen, activeEngine: engine };
      });
      // A selection restores the session whether or not its messages are
      // already cached (e.g. reopened after a tab close): the hook fires once
      // per frontend lifetime, before any load.
      if (!get().restoredSessionKeys[key]) {
        set((s) => ({ restoredSessionKeys: { ...s.restoredSessionKeys, [key]: true } }));
        dispatchSessionRestored({ ...sessionLifecycleBase(get, { engine, sessionId, workspacePath }), sessionId });
      }
      emitSessionActivated(engine, sessionId);
      const existing = get().bySession[key];
      const remoteHistory = get().sessions.some((session) => session.engine === engine && session.sessionId === sessionId && session.workspacePath === workspacePath && session.remote);
      if (existing?.loading || existing?.historyCache?.refreshing) return;
      if (existing?.messages.length && !existing.historyCache) return;
      const initialSettledRuns = existing?.settledRunIds;
      let initialTailSeq = Math.max(0, ...(existing?.messages ?? []).map((message) => message.seq));
      const hasNewLiveRows = (current: typeof EMPTY_SESSION) => current.streaming ||
        current.settledRunIds !== initialSettledRuns || current.messages.some((message) => message.seq > initialTailSeq);
      patchSession(set, key, {
        loading: !existing?.historyCache,
        error: null,
        historyCache: existing?.historyCache ? { ...existing.historyCache, refreshing: true, refreshError: null } : null,
      });
      // 并行取计划审批历史:失败静默降级为无历史计划,绝不阻塞会话加载。
      const plansPromise = ipc
        .listPlanReviews(engine, sessionId)
        .catch(() => null);
      try {
        if (!existing?.historyCache) {
          const cached = await loadCachedHistoryPage(engine, sessionId, workspacePath, 100).catch(() => null);
          if (cached) {
            set((state) => {
              const current = state.bySession[key] ?? EMPTY_SESSION;
              const preserveLive = hasNewLiveRows(current);
              if (!preserveLive) initialTailSeq = Math.max(0, ...cached.page.messages.map((message) => message.seq));
              return { bySession: { ...state.bySession, [key]: {
                ...current,
                ...(preserveLive ? {} : {
                  messages: cached.page.messages, subagentHistory: cached.page.subagentHistory,
                  nextBefore: cached.page.nextBefore,
                  usage: [...cached.page.messages].reverse().find((message) => message.usage)?.usage ?? current.usage,
                }),
                loading: false,
                historyCache: { cachedAt: cached.cachedAt, refreshing: true, refreshError: null },
              } } };
            });
          }
        }
        const page = await loadHistoryPage(engine, sessionId, workspacePath, 100);
        set((state) => {
          const current = state.bySession[key] ?? EMPTY_SESSION;
          return { bySession: { ...state.bySession, [key]: {
            ...current,
            ...(hasNewLiveRows(current) ? {} : {
              messages: current.historyCache ? refreshCachedMessages(current.messages, page.messages) : page.messages,
              subagentHistory: page.subagentHistory,
              nextBefore: current.historyCache && current.messages.length ? current.nextBefore : page.nextBefore,
              usage: [...page.messages].reverse().find((message) => message.usage)?.usage ?? current.usage,
            }),
            loading: false,
            historyCache: remoteHistory ? { cachedAt: Date.now(), refreshing: false, refreshError: null, updated: true } : null,
            // A runtime error raised during loading belongs to that turn.
          } } };
        });
        // 历史页应用后再并入计划卡片(按 planId+revision 去重;活跃等待点
        // 可否恢复由后端状态保证,前端只按 record.status 渲染)。
        const plans = await plansPromise;
        if (plans && plans.length > 0) {
          set((s) => {
            const cur = s.bySession[key];
            if (!cur) return {};
            const messages = mergePlanReviewHistory(cur.messages, plans);
            if (!messages) return {};
            return {
              bySession: { ...s.bySession, [key]: { ...cur, messages } },
            };
          });
        }
      } catch (error) {
        set((state) => {
          const current = state.bySession[key] ?? EMPTY_SESSION;
          return { bySession: { ...state.bySession, [key]: {
            ...current, loading: false,
            ...(current.historyCache ? {
              historyCache: { ...current.historyCache, refreshing: false, refreshError: errorText(error), updated: false },
            } : { error: current.error ?? String(error) }),
          } } };
        });
      }
    },

    loadEarlier: async (key) => {
      const { active, bySession, openTabs } = get();
      // 分屏里每格各自向上加载历史：不带 key 时仍按激活会话。
      const targetKey =
        key ??
        (active
          ? sessionKey(active.engine, active.sessionId, active.workspacePath)
          : "");
      if (!targetKey) return;
      const tab = openTabs.find(
        (t) => sessionKey(t.engine, t.sessionId, t.workspacePath) === targetKey,
      );
      if (!tab?.sessionId) return;
      const state = bySession[targetKey];
      if (!state?.nextBefore || state.loading) return;
      patchSession(set, targetKey, { loading: true });
      try {
        const cached = await loadCachedHistoryPage(
          tab.engine,
          tab.sessionId,
          tab.workspacePath,
          100,
          state.nextBefore,
        ).catch(() => null);
        const page = cached?.page ?? await loadHistoryPage(tab.engine, tab.sessionId, tab.workspacePath, 100, state.nextBefore);
        const current = get().bySession[targetKey] ?? EMPTY_SESSION;
        const existingSeqs = new Set(current.messages.map((message) => message.seq));
        patchSession(set, targetKey, {
          messages: [
            ...page.messages.filter((message) => !existingSeqs.has(message.seq)),
            ...current.messages,
          ],
          nextBefore: page.nextBefore,
          subagentHistory: page.subagentHistory,
          loading: false,
          ...(cached ? { historyCache: current.historyCache ?? { cachedAt: cached.cachedAt, refreshing: false, refreshError: null } } : {}),
        });
      } catch (error) {
        patchSession(set, targetKey, { loading: false, error: errorText(error) });
      }
    },

    archiveSession: async (session) => {
      const { engine, sessionId, workspacePath } = session;
      const key = sessionKey(engine, sessionId, workspacePath);
      if (get().streamingByKey[key]) {
        set({ actionError: i18n.t("chat.archiveRunning") });
        return;
      }
      try {
        await ipc.archiveSession(session);
      } catch (error) {
        set({ actionError: errorText(error) });
        return;
      }
      removeCachedSshSessionMeta(get().workspaces, engine, sessionId, workspacePath);
      set((s) => {
        const bySession = { ...s.bySession };
        const drafts = { ...s.drafts };
        delete bySession[key];
        delete drafts[key];
        return {
          sessions: s.sessions.filter(
            (item) => !(item.engine === engine && item.sessionId === sessionId),
          ),
          archivedSessionKeys: { ...s.archivedSessionKeys, [key]: true },
          bySession,
          drafts,
          unseen: omitKey(s.unseen, key),
          actionError: null,
        };
      });
      forgetClosedTab(key);
      const tab = get().openTabs.find(
        (item) => item.engine === engine && item.sessionId === sessionId,
      );
      if (tab) get().closeTab(engine, sessionId, tab.workspacePath);
      else clearScopedContributions(set, engine, sessionId, workspacePath);
    },

    deleteSession: async (engine, sessionId) => {
      // 远程(插件会话源,如 WSL 发行版内 CLI)会话没有本地 db 行,本地
      // delete_session 只会 "session not found";走远程通道删 remotePath。
      const meta = get().sessions.find(
        (x) => x.engine === engine && x.sessionId === sessionId,
      );
      try {
        if (meta?.remote && meta.remotePath) {
          await ipc.deleteRemoteSession(meta.workspacePath, engine, sessionId, meta.remotePath);
        } else {
          await ipc.deleteSession(engine, sessionId);
        }
      } catch (error) {
        set({ actionError: errorText(error) });
        return;
      }
      set({ actionError: null });
      removeCachedSshSessionMeta(get().workspaces, engine, sessionId, meta?.workspacePath);
      const key = sessionKey(engine, sessionId, "");
      const tab = get().openTabs.find(
        (t) => t.engine === engine && t.sessionId === sessionId,
      );
      // The session is gone: its remembered contributions go with it.
      clearScopedContributions(set, engine, sessionId, "");
      set((s) => {
        // Permanent delete: the cached session state is dead weight.
        const bySession = { ...s.bySession };
        const drafts = { ...s.drafts };
        const streamingByKey = { ...s.streamingByKey };
        const retryingByKey = { ...s.retryingByKey };
        delete bySession[key];
        delete drafts[key];
        // Deleting a running session must clear its flat-map flags too:
        // nothing re-scans a deleted key later (archive is swept by
        // refreshSessions; a delete is gone for good).
        delete streamingByKey[key];
        delete retryingByKey[key];
        return {
          sessions: s.sessions.filter(
            (x) => !(x.engine === engine && x.sessionId === sessionId),
          ),
          bySession,
          drafts,
          streamingByKey,
          retryingByKey,
          unseen: omitKey(s.unseen, key),
        };
      });
      // The closed-tab reopen cache must not keep the dead key either.
      forgetClosedTab(key);
      if (tab) {
        // removeTab activates the neighboring tab when the deleted one was active.
        removeTab(engine, sessionId, tab.workspacePath);
      } else {
        set((s) => ({
          active:
            s.active?.sessionId === sessionId && s.active.engine === engine
              ? null
              : s.active,
        }));
      }
    },

    pinSession: async (engine, sessionId, pinned) => {
      try {
        await ipc.pinSession(engine, sessionId, pinned);
        await get().refreshSessions();
        set({ actionError: null });
      } catch (error) {
        set({ actionError: errorText(error) });
      }
    },

    renameSession: async (engine, sessionId, title) => {
      try {
        await ipc.renameSession(engine, sessionId, title);
        await get().refreshSessions();
        set({ actionError: null });
      } catch (error) {
        set({ actionError: errorText(error) });
      }
    },
  };
}
