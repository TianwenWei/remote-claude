import type { SessionMeta, Workspace } from "@/lib/ipc";
import { sshWorkspaceMetaOf } from "@/lib/ssh-workspace";
import { writeStored } from "@/lib/storage";

export const SSH_SESSION_CATALOG_KEY = "ccgui-remote.sshSessionCatalog:v1";
const MAX_PER_WORKSPACE = 500;
const MAX_TOTAL = 2000;
type Catalog = Record<string, SessionMeta[]>;
const revisions = new Map<string, number>();

function partition(workspace: Workspace): string | null {
  const ssh = sshWorkspaceMetaOf(workspace);
  return ssh ? JSON.stringify([ssh.host, ssh.user, ssh.port, workspace.path]) : null;
}

/** Persist only the routing and sidebar metadata needed to reopen a local
 * transcript snapshot. No SSH identity path, transcript text or preview. */
function thinMeta(raw: unknown, workspace: Workspace): SessionMeta | null {
  if (!raw || typeof raw !== "object") return null;
  const value = raw as Partial<SessionMeta>;
  if (typeof value.engine !== "string" || !value.engine || value.engine.length > 64 ||
      typeof value.sessionId !== "string" || !value.sessionId || value.sessionId.length > 256 ||
      typeof value.remotePath !== "string" || !value.remotePath.startsWith("/") || value.remotePath.length > 4096) return null;
  const text = (input: unknown, limit = 512) => typeof input === "string" ? input.slice(0, limit) : null;
  const number = (input: unknown) => typeof input === "number" && Number.isFinite(input) ? input : null;
  return {
    engine: value.engine, sessionId: value.sessionId, workspacePath: workspace.path,
    filePath: value.remotePath, remote: true, remotePath: value.remotePath,
    fileSize: number(value.fileSize) ?? 0, fileMtimeMs: number(value.fileMtimeMs) ?? 0,
    title: text(value.title) ?? value.sessionId.slice(0, 8), preview: "",
    createdAt: number(value.createdAt), updatedAt: number(value.updatedAt),
    messageCount: number(value.messageCount) ?? 0, pinned: value.pinned === true,
    customTitle: text(value.customTitle), model: text(value.model, 256),
    effort: text(value.effort, 64), provider: text(value.provider, 256),
  };
}

function readCatalog(workspaces: Workspace[]): Catalog {
  let raw: unknown;
  try { raw = JSON.parse(localStorage.getItem(SSH_SESSION_CATALOG_KEY) ?? "{}"); } catch { raw = {}; }
  const catalog: Catalog = {};
  if (!raw || typeof raw !== "object" || Array.isArray(raw)) return catalog;
  for (const workspace of workspaces) {
    const key = partition(workspace);
    const rows = key ? (raw as Record<string, unknown>)[key] : null;
    if (!key || !Array.isArray(rows)) continue;
    catalog[key] = rows.slice(0, MAX_PER_WORKSPACE).flatMap((row) => {
      const meta = thinMeta(row, workspace);
      return meta ? [meta] : [];
    });
  }
  return catalog;
}

function saveCatalog(workspaces: Workspace[], catalog: Catalog) {
  const bounded: Catalog = {};
  let remaining = MAX_TOTAL;
  for (const workspace of workspaces) {
    const key = partition(workspace);
    if (!key || !catalog[key]?.length || remaining <= 0) continue;
    const rows = catalog[key].slice(0, Math.min(MAX_PER_WORKSPACE, remaining));
    bounded[key] = rows;
    remaining -= rows.length;
  }
  writeStored(SSH_SESSION_CATALOG_KEY, JSON.stringify(bounded));
}

export function cachedSshSessionMetas(workspaces: Workspace[]): SessionMeta[] {
  const catalog = readCatalog(workspaces);
  // Removed workspaces and old connection identities must not accumulate.
  saveCatalog(workspaces, catalog);
  return Object.values(catalog).flat().slice(0, MAX_TOTAL);
}

export function sshSessionCatalogRevision(workspace: Workspace): number {
  return revisions.get(partition(workspace) ?? "") ?? 0;
}

export function cacheSshSessionMetas(workspaces: Workspace[], workspace: Workspace, sessions: SessionMeta[], expectedRevision?: number) {
  const key = partition(workspace);
  if (!key || (expectedRevision !== undefined && sshSessionCatalogRevision(workspace) !== expectedRevision)) return;
  const catalog = readCatalog(workspaces);
  catalog[key] = [...sessions].sort((a, b) => (b.updatedAt ?? 0) - (a.updatedAt ?? 0))
    .slice(0, MAX_PER_WORKSPACE).flatMap((session) => {
      const meta = thinMeta(session, workspace);
      return meta ? [meta] : [];
    });
  // A successful empty list deliberately removes the previous partition.
  saveCatalog(workspaces, catalog);
}

export function removeCachedSshSessionMeta(workspaces: Workspace[], engine: string, sessionId: string, workspacePath?: string) {
  const catalog = readCatalog(workspaces);
  for (const workspace of workspaces) {
    if (workspacePath && workspace.path !== workspacePath) continue;
    const key = partition(workspace);
    if (key) {
      revisions.set(key, sshSessionCatalogRevision(workspace) + 1);
      if (catalog[key]) catalog[key] = catalog[key].filter((meta) => meta.engine !== engine || meta.sessionId !== sessionId);
    }
  }
  saveCatalog(workspaces, catalog);
}
