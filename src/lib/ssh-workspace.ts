import type { SshWorkspaceMeta, Workspace } from "./ipc";

/** Native SSH metadata selects the remote transport for the workspace's
 * canonical POSIX path. Private-key contents never cross IPC. */
export function sshWorkspaceMetaOf(workspace?: Workspace): SshWorkspaceMeta | null {
  const raw = workspace?.meta?.ssh;
  if (!raw || typeof raw !== "object" || Array.isArray(raw)) return null;
  const value = raw as Record<string, unknown>;
  if (typeof value.host !== "string" || !value.host ||
      typeof value.user !== "string" || !value.user ||
      typeof value.workspace !== "string" || !value.workspace ||
      typeof value.port !== "number" || !Number.isInteger(value.port) ||
      value.port < 1 || value.port > 65535 ||
      (value.identityFile !== null && typeof value.identityFile !== "string") ||
      !value.enginePaths || typeof value.enginePaths !== "object" || Array.isArray(value.enginePaths)) {
    return null;
  }
  const enginePaths = Object.fromEntries(
    Object.entries(value.enginePaths).filter(([, path]) => typeof path === "string" && path.length > 0),
  ) as Record<string, string>;
  return { host: value.host, user: value.user, port: value.port,
    identityFile: value.identityFile, workspace: value.workspace, enginePaths } as SshWorkspaceMeta;
}

/** Exact root or child path, with a separator boundary (not a string-prefix
 * match against an unrelated local directory such as /srv/project-copy). */
export function isSshWorkspacePath(workspaces: Workspace[], path: string): boolean {
  return workspaces.some((workspace) => {
    if (!sshWorkspaceMetaOf(workspace)) return false;
    const root = workspace.path.replace(/\/+$/, "");
    return path === workspace.path || path.startsWith(`${root}/`);
  });
}
