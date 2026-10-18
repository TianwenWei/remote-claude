import { useEffect, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import Folder from "lucide-react/dist/esm/icons/folder";
import ArrowUp from "lucide-react/dist/esm/icons/arrow-up";
import RefreshCw from "lucide-react/dist/esm/icons/refresh-cw";
import Server from "lucide-react/dist/esm/icons/server";
import FolderPlus from "lucide-react/dist/esm/icons/folder-plus";
import { SettingsCard, SettingsRow, SettingsSectionLabel } from "@/components/application/settings/settings-rows";
import { ActionFeedbackIcon, useActionFeedback } from "@/components/base/action-feedback";
import { Button } from "@/components/base/buttons/button";
import { Checkbox } from "@/components/base/checkbox/checkbox";
import { Input } from "@/components/base/input/input";
import { ipc, type SshConnectionInput, type SshConnectionProbe, type SshHostKeyInspection, type Workspace } from "@/lib/ipc";
import { sshWorkspaceMetaOf } from "@/lib/ssh-workspace";
import { errorText } from "@/lib/errors";
import { pickFile } from "@/lib/platform";
import { isWeb } from "@/lib/transport";
import { useChatStore } from "@/features/chat/store";
import { dismissCenterSurfaces } from "@/features/chat/center-surfaces";

interface VerifiedDirectory { signature: string; probe: SshConnectionProbe }
interface InspectedHost { signature: string; inspection: SshHostKeyInspection; directory: string }
const signatureOf = (connection: SshConnectionInput, path: string) => JSON.stringify([connection, path]);

/** Native SSH setup lives next to workspace management. No browser bridge
 * may inspect this machine's SSH identities or register remote hosts. */
export function SshWorkspacesCard({
  workspaces,
  onRegistered,
}: {
  workspaces: Workspace[];
  onRegistered: (workspace: Workspace) => Promise<void>;
}) {
  const { t } = useTranslation();
  const [initial] = useState(() => {
    const activePath = useChatStore.getState().active?.workspacePath;
    const ssh = sshWorkspaceMetaOf(workspaces.find((workspace) => workspace.path === activePath))
      ?? workspaces.map(sshWorkspaceMetaOf).find(Boolean);
    const creating = new URLSearchParams(window.location.hash.split("?")[1]).get("action") === "new-ssh-project";
    return { ssh, path: ssh ? (creating ? ssh.workspace.replace(/\/[^/]+\/?$/, "") || "/" : ssh.workspace) : "~" };
  });
  const [host, setHost] = useState(initial.ssh?.host ?? "");
  const [user, setUser] = useState(initial.ssh?.user ?? "");
  const [port, setPort] = useState(String(initial.ssh?.port ?? 22));
  const [identityFile, setIdentityFile] = useState(initial.ssh?.identityFile ?? "");
  const [path, setPath] = useState(initial.path);
  const [name, setName] = useState("");
  const [verified, setVerified] = useState<VerifiedDirectory | null>(null);
  const [hostKey, setHostKey] = useState<InspectedHost | null>(null);
  const [selectedFingerprint, setSelectedFingerprint] = useState("");
  const [fingerprintVerified, setFingerprintVerified] = useState(false);
  const [busy, setBusy] = useState<"probe" | "trust" | "key" | "add" | "create" | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [notice, setNotice] = useState<string | null>(null);
  const inFlight = useRef(false);
  const generation = useRef(0);
  const probeFeedback = useActionFeedback({ spin: true });
  useEffect(() => () => { generation.current += 1; }, []);

  const connection: SshConnectionInput = {
    host: host.trim(), user: user.trim(), port: Number(port), identityFile: identityFile.trim() || null,
  };
  const validIdentity = !connection.identityFile || (connection.identityFile.startsWith("/") && !/[\u0000-\u001f\u007f-\u009f]/.test(connection.identityFile));
  const valid = connection.host.length > 0 && connection.user.length > 0 &&
    /^\d+$/.test(port) && Number.isInteger(connection.port) && connection.port > 0 && connection.port <= 65535 && path.trim().length > 0 && validIdentity;
  const hostSignature = JSON.stringify([connection.host, connection.port, connection.user]);
  const currentHost = hostKey?.signature === hostSignature ? hostKey : null;
  const current = verified?.signature === signatureOf(connection, path.trim()) ? verified.probe : null;
  const missingDependencies = current ? Object.entries(current.dependencies).filter(([key, value]) => key !== "git" && !value.available).map(([key]) => key) : [];
  const ready = current?.claudeAvailable && current.claudeAuth.status === "authenticated" && missingDependencies.length === 0;
  const projectName = name.trim();
  const validName = projectName.length > 0 && projectName !== "." && projectName !== ".." && !/[\/\\\u0000-\u001f\u007f-\u009f]/.test(projectName);
  const remotes = workspaces.flatMap((workspace) => {
    const ssh = sshWorkspaceMetaOf(workspace);
    return ssh ? [{ workspace, ssh }] : [];
  });

  const edit = (setter: (value: string) => void) => (value: string) => {
    setter(value);
    setVerified(null);
    setHostKey(null);
    setSelectedFingerprint("");
    setFingerprintVerified(false);
    setError(null);
    setNotice(null);
  };

  const chooseIdentity = async () => {
    if (inFlight.current) return;
    inFlight.current = true;
    const requestGeneration = generation.current;
    setBusy("key");
    setError(null);
    try {
      const selected = await pickFile(t("settings.sshChooseIdentity"), []);
      if (requestGeneration !== generation.current || selected === null) return;
      if (!selected.startsWith("/") || /[\u0000-\u001f\u007f-\u009f]/.test(selected)) throw new Error(t("settings.sshIdentityInvalid"));
      edit(setIdentityFile)(selected);
    } catch (cause) {
      if (requestGeneration === generation.current) setError(t("settings.sshIdentityFailed", { error: errorText(cause) }));
    } finally {
      if (requestGeneration === generation.current) setBusy(null);
      inFlight.current = false;
    }
  };

  const showInspection = (inspection: SshHostKeyInspection, directory: string) => {
    setHostKey({ signature: hostSignature, inspection, directory });
    setSelectedFingerprint(inspection.keys[0]?.fingerprint ?? "");
    setFingerprintVerified(false);
  };

  const probe = async (directory = path.trim()) => {
    if (!valid || inFlight.current) return;
    inFlight.current = true;
    const requestGeneration = generation.current;
    setBusy("probe");
    setError(null);
    setNotice(null);
    setVerified(null);
    try {
      await probeFeedback.start(async () => {
        const inspection = currentHost?.inspection.status === "known"
          ? currentHost.inspection : await ipc.inspectSshHostKey(connection.host, connection.port, connection.user);
        if (requestGeneration !== generation.current) return;
        showInspection(inspection, directory);
        if (inspection.status !== "known") return;
        const result = await ipc.probeSshConnection(connection, directory);
        if (requestGeneration !== generation.current) return;
        setPath(result.canonicalPath);
        setVerified({ signature: signatureOf(connection, result.canonicalPath), probe: result });
      });
    } catch (cause) {
      if (requestGeneration === generation.current) setError(t("settings.sshConnectionFailed", { error: errorText(cause) }));
    } finally {
      if (requestGeneration === generation.current) setBusy(null);
      inFlight.current = false;
    }
  };

  const trust = async () => {
    if (!valid || currentHost?.inspection.status !== "new" || !fingerprintVerified || inFlight.current ||
      !currentHost.inspection.keys.some((key) => key.fingerprint === selectedFingerprint)) return;
    inFlight.current = true;
    const requestGeneration = generation.current;
    const directory = currentHost.directory;
    setBusy("trust");
    setError(null);
    setNotice(null);
    setVerified(null);
    try {
      const inspection = await ipc.trustSshHostKey(connection.host, connection.port, selectedFingerprint, connection.user);
      if (requestGeneration !== generation.current) return;
      showInspection(inspection, directory);
      if (inspection.status !== "known") return;
      const result = await ipc.probeSshConnection(connection, directory);
      if (requestGeneration !== generation.current) return;
      setPath(result.canonicalPath);
      setVerified({ signature: signatureOf(connection, result.canonicalPath), probe: result });
    } catch (cause) {
      if (requestGeneration === generation.current) setError(t("settings.sshTrustFailed", { error: errorText(cause) }));
    } finally {
      if (requestGeneration === generation.current) setBusy(null);
      inFlight.current = false;
    }
  };

  const add = async () => {
    if (!ready || !current || inFlight.current) return;
    inFlight.current = true;
    const requestGeneration = generation.current;
    setBusy("add");
    setError(null);
    setNotice(null);
    try {
      const workspace = await ipc.addSshWorkspace(connection, current.canonicalPath);
      await onRegistered(workspace);
      if (requestGeneration !== generation.current) return;
      setNotice(t("settings.sshWorkspaceAdded", { name: workspace.name }));
    } catch (cause) {
      if (requestGeneration === generation.current) setError(t("settings.sshRegistrationFailed", { error: errorText(cause) }));
    } finally {
      if (requestGeneration === generation.current) setBusy(null);
      inFlight.current = false;
    }
  };

  const openProject = (workspace: Workspace) => {
    dismissCenterSurfaces();
    useChatStore.getState().startNewChat(workspace.path);
    window.location.hash = "#/";
  };

  const create = async () => {
    if (!ready || !current || !validName || inFlight.current) return;
    inFlight.current = true;
    const requestGeneration = generation.current;
    setBusy("create");
    setError(null);
    setNotice(null);
    let created: Workspace | null = null;
    try {
      created = await ipc.createSshWorkspace(connection, current.canonicalPath, projectName);
      await onRegistered(created);
      if (requestGeneration !== generation.current) return;
      setName("");
      setNotice(t("settings.sshWorkspaceCreated", { name: created.name }));
      openProject(created);
    } catch (cause) {
      if (requestGeneration === generation.current) {
        if (created) setName("");
        setError(t(created ? "settings.sshCreatedRefreshFailed" : "settings.sshCreationFailed", { error: errorText(cause), name: created?.name }));
      }
    } finally {
      if (requestGeneration === generation.current) setBusy(null);
      inFlight.current = false;
    }
  };

  if (isWeb) return null;
  return (
    <section className="flex w-full flex-col gap-2" aria-label={t("settings.sshWorkspaces")}>
      <SettingsSectionLabel anchor="sshWorkspaces">{t("settings.sshWorkspaces")}</SettingsSectionLabel>
      <SettingsCard>
        <div className="flex flex-col gap-4 py-3 pr-3">
          <p className="text-body-2-regular text-text-secondary">{t("settings.sshWorkspacesDesc")}</p>
          <div className="rounded-lg border border-separator-border p-3">
            <p className="text-body-2-medium text-text-primary">{t("settings.sshPrepareTitle")}</p>
            <ol className="mt-2 list-decimal space-y-1 pl-5 text-body-2-regular text-text-secondary">
              <li>{t("settings.sshPrepareServer")}</li>
              <li>{t("settings.sshPrepareKey")}</li>
              <li>{t("settings.sshPrepareFingerprint")}</li>
            </ol>
          </div>
          <div className="grid grid-cols-1 gap-3 sm:grid-cols-2">
            <Input label={t("settings.sshHost")} value={host} onChange={edit(setHost)} isDisabled={busy !== null} autoComplete="off" />
            <Input label={t("settings.sshUser")} value={user} onChange={edit(setUser)} isDisabled={busy !== null} autoComplete="off" />
            <Input label={t("settings.sshPort")} value={port} onChange={edit(setPort)} isDisabled={busy !== null} inputMode="numeric" />
            <div className="flex flex-col items-start gap-2">
              <Input label={t("settings.sshIdentityFile")} value={identityFile} onChange={edit(setIdentityFile)} isDisabled={busy !== null}
                isInvalid={!validIdentity} hint={validIdentity ? t("settings.sshIdentityHint") : t("settings.sshIdentityInvalid")} autoComplete="off" />
              <Button size="small" variant="secondary" leadingIcon={Folder} disabled={busy !== null} onClick={() => void chooseIdentity()}>{t("settings.sshChooseIdentity")}</Button>
            </div>
          </div>
          <Input label={t("settings.sshProjectPath")} value={path} onChange={edit(setPath)} isDisabled={busy !== null} hint={t("settings.sshProjectPathHint")} autoComplete="off" />
          <div className="flex flex-wrap items-center gap-2">
            <Button size="small" disabled={!valid || busy !== null} onClick={() => void probe()}>
              <ActionFeedbackIcon icon={RefreshCw} feedback={probeFeedback.feedback} spin />
              {busy === "probe" ? t("settings.sshConnecting") : t("settings.sshTestConnection")}
            </Button>
            <Button size="small" variant="secondary" leadingIcon={Server} disabled={!ready || busy !== null} onClick={() => void add()}>
              {busy === "add" ? t("settings.sshAdding") : t("settings.sshAddProject")}
            </Button>
          </div>
          {currentHost?.inspection.status === "known" && <p className="text-caption-1-regular text-text-tertiary">{t("settings.sshHostKnown")}</p>}
          {currentHost && currentHost.inspection.status !== "known" && (
            <div className="flex flex-col gap-3 rounded-lg border border-separator-border p-3">
              <p role={currentHost.inspection.status === "changed" ? "alert" : "status"} className="text-body-2-medium text-text-primary">
                {t(currentHost.inspection.status === "changed" ? "settings.sshHostChanged" : "settings.sshHostNew")}
              </p>
              {currentHost.inspection.status === "changed" && currentHost.inspection.knownFingerprints.length > 0 && (
                <p className="break-all text-caption-1-regular text-text-secondary">{t("settings.sshKnownFingerprints", { fingerprints: currentHost.inspection.knownFingerprints.join(", ") })}</p>
              )}
              <fieldset className="flex min-w-0 flex-col gap-2" disabled={busy !== null || currentHost.inspection.status === "changed"}>
                <legend className="mb-2 text-body-2-regular text-text-secondary">{t("settings.sshHostFingerprints")}</legend>
                {currentHost.inspection.keys.map((key) => (
                  <label key={`${key.algorithm}:${key.fingerprint}`} className="flex items-start gap-2 text-body-2-regular text-text-secondary">
                    <input type="radio" name="ssh-host-fingerprint" className="mt-1 shrink-0" checked={selectedFingerprint === key.fingerprint}
                      onChange={() => { setSelectedFingerprint(key.fingerprint); setFingerprintVerified(false); }} />
                    <span className="min-w-0 break-all">{key.algorithm} · <code>{key.fingerprint}</code></span>
                  </label>
                ))}
              </fieldset>
              {currentHost.inspection.status === "new" && <>
                <Checkbox size="sm" isSelected={fingerprintVerified} onChange={setFingerprintVerified} isDisabled={busy !== null}>{t("settings.sshFingerprintVerified")}</Checkbox>
                <Button size="small" variant="secondary" className="self-start" disabled={!selectedFingerprint || !fingerprintVerified || busy !== null} onClick={() => void trust()}>
                  {busy === "trust" ? t("settings.sshTrusting") : t("settings.sshTrustHost")}
                </Button>
              </>}
            </div>
          )}
          {current && (
            <div className="flex flex-col gap-2">
              <p role="status" className="text-body-2-regular text-text-secondary">
                {!current.claudeAvailable ? t("settings.sshClaudeMissing")
                  : missingDependencies.length > 0 ? t("settings.sshDependenciesMissing", { names: missingDependencies.join(", ") })
                    : current.claudeAuth.status !== "authenticated" ? t(current.claudeAuth.status === "unauthenticated" ? "settings.sshClaudeUnauthenticated" : "settings.sshClaudeAuthUnknown")
                      : t("settings.sshConnected", { version: current.claudeVersion ?? "Claude Code" })}
              </p>
              {!current.dependencies.git.available && <p className="text-body-2-regular text-text-secondary">{t("settings.sshGitMissing")}</p>}
              <div className="flex items-center gap-2">
                <Button iconOnly size="small" variant="ghost" leadingIcon={ArrowUp} aria-label={t("settings.sshParentFolder")} title={t("settings.sshParentFolder")} disabled={busy !== null || current.canonicalPath === "/"} onClick={() => void probe(`${current.canonicalPath}/..`)} />
                <span className="min-w-0 break-all text-body-2-regular text-text-secondary">{current.canonicalPath}</span>
              </div>
              <div className="max-h-52 overflow-auto rounded-lg border border-separator-border" aria-label={t("settings.sshFolders")}>
                {current.directories.length === 0
                  ? <p className="px-3 py-2 text-body-2-regular text-text-tertiary">{t("settings.sshNoFolders")}</p>
                  : current.directories.map((directory) => (
                    <Button key={directory.path} size="small" variant="ghost" leadingIcon={Folder} className="w-full justify-start" disabled={busy !== null} onClick={() => void probe(directory.path)}>
                      {directory.name}
                    </Button>
                  ))}
              </div>
            </div>
          )}
          <div className="flex flex-col gap-3 border-t border-separator-border pt-4">
            <p className="text-body-2-medium text-text-primary">{t("settings.sshNewProject")}</p>
            <p className="text-body-2-regular text-text-secondary">
              {current ? t("settings.sshCreateParent", { path: current.canonicalPath }) : t("settings.sshCreateConnectFirst")}
            </p>
            <Input label={t("settings.sshProjectName")} value={name} onChange={(value) => { setName(value); setError(null); setNotice(null); }}
              isDisabled={busy !== null} autoComplete="off" isInvalid={name.length > 0 && !validName}
              hint={name.length > 0 && !validName ? t("settings.sshProjectNameInvalid") : t("settings.sshProjectNameHint")} />
            {validName && current && <p className="break-all text-caption-1-regular text-text-tertiary">{current.canonicalPath.replace(/\/$/, "")}/{projectName}</p>}
            <Button size="small" leadingIcon={FolderPlus} className="self-start" disabled={!ready || !validName || busy !== null} onClick={() => void create()}>
              {busy === "create" ? t("settings.sshCreating") : t("settings.sshCreateProject")}
            </Button>
          </div>
          {error && <p role="alert" className="break-words text-body-2-regular text-text-error-primary">{error}</p>}
          {notice && <p role="status" className="text-body-2-regular text-text-secondary">{notice}</p>}
        </div>
        {remotes.map(({ workspace, ssh }) => (
          <SettingsRow key={workspace.id} label={workspace.name} description={`${ssh.user}@${ssh.host}:${ssh.port} · ${ssh.workspace}`}>
            <Button size="small" variant="ghost" onClick={() => openProject(workspace)}>{t("settings.sshOpenProject")}</Button>
          </SettingsRow>
        ))}
      </SettingsCard>
    </section>
  );
}
