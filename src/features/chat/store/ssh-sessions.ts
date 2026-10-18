import { ipc, type SessionMeta, type Workspace } from "@/lib/ipc";
import { sshWorkspaceMetaOf } from "@/lib/ssh-workspace";
import { errorText } from "@/lib/errors";
import { cacheSshSessionMetas, cachedSshSessionMetas, sshSessionCatalogRevision } from "./ssh-session-catalog";
import { sessionKey } from "./persistence";

interface PendingCatalogRead {
  revision: number;
  promise: Promise<SessionMeta[]>;
}

const pendingCatalogReads = new Map<string, PendingCatalogRead>();

/** Startup, engine changes and session notifications share the same SSH
 * read. Completed reads are not cached here: explicit refresh stays fresh. */
function readWorkspaceCatalog(workspace: Workspace, revision: number): Promise<SessionMeta[]> {
  const ssh = sshWorkspaceMetaOf(workspace)!;
  const source = JSON.stringify([ssh.host, ssh.user, ssh.port, ssh.identityFile, ssh.workspace, workspace.path]);
  const pending = pendingCatalogReads.get(source);
  if (pending?.revision === revision) return pending.promise;
  const read: PendingCatalogRead = {
    revision,
    promise: ipc.listSshSessions(workspace.path).finally(() => {
      if (pendingCatalogReads.get(source) === read) pendingCatalogReads.delete(source);
    }),
  };
  pendingCatalogReads.set(source, read);
  return read.promise;
}

/** A failed SSH refresh must retain previously loaded history, including
 * unloaded transcripts. The caller surfaces errors without blanking tabs. */
export async function listSshSessionMetas(
  workspaces: Workspace[],
  previous: SessionMeta[],
  excluded: Record<string, boolean> = {},
): Promise<{ sessions: SessionMeta[]; errors: Array<{ name: string; error: string }> }> {
  const cached = cachedSshSessionMetas(workspaces);
  const visible = (sessions: SessionMeta[]) => sessions.filter((session) => !excluded[sessionKey(session.engine, session.sessionId, session.workspacePath)]);
  const results = await Promise.all(workspaces.filter((workspace) => sshWorkspaceMetaOf(workspace)).map(async (workspace) => {
    const revision = sshSessionCatalogRevision(workspace);
    const currentCached = () => visible(cachedSshSessionMetas(workspaces).filter((session) => session.workspacePath === workspace.path));
    try {
      const sessions = visible(await readWorkspaceCatalog(workspace, revision));
      // Delete/archive invalidates a pending snapshot for both persistence
      // and the sidebar. A later caller can start a new revision immediately.
      if (sshSessionCatalogRevision(workspace) !== revision) return { sessions: currentCached(), errors: [] };
      cacheSshSessionMetas(workspaces, workspace, sessions, revision);
      return { sessions, errors: [] };
    } catch (cause) {
      if (sshSessionCatalogRevision(workspace) !== revision) return { sessions: currentCached(), errors: [] };
      const retained = previous.filter((session) => session.workspacePath === workspace.path && session.remote);
      return {
        sessions: visible(retained.length ? retained : cached.filter((session) => session.workspacePath === workspace.path)),
        errors: [{ name: workspace.name, error: errorText(cause) }],
      };
    }
  }));
  return { sessions: results.flatMap((result) => result.sessions), errors: results.flatMap((result) => result.errors) };
}
