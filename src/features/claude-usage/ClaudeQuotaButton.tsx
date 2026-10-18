import { useEffect, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { Button as AriaButton, Dialog, DialogTrigger, Popover } from "react-aria-components";
import Gauge from "lucide-react/dist/esm/icons/gauge";
import RefreshCw from "lucide-react/dist/esm/icons/refresh-cw";
import { cx } from "@/utils/cx";
import { useDismissOnOutsidePress, useTriggerToggle } from "@/utils/use-dismiss-on-outside-press";
import type { ClaudeUsageWindow, Workspace } from "@/lib/ipc";
import { sshWorkspaceMetaOf } from "@/lib/ssh-workspace";
import { remainingPercent, useClaudeUsageStore } from "./store";

export function ClaudeQuotaButton({ workspacePath, workspaces, streaming }: {
  workspacePath: string;
  workspaces: Workspace[];
  streaming: boolean;
}) {
  const workspace = workspaces.filter((item) => sshWorkspaceMetaOf(item) &&
    (item.path === workspacePath || workspacePath.startsWith(`${item.path.replace(/\/+$/, "")}/`)))
    .sort((left, right) => right.path.length - left.path.length)[0];
  const connection = sshWorkspaceMetaOf(workspace);
  if (!connection) return null;
  const account = JSON.stringify([connection.host, connection.port, connection.user, connection.identityFile]);
  return <QuotaDetails key={account} account={account} workspacePath={workspacePath} streaming={streaming} />;
}

function QuotaDetails({ account, workspacePath, streaming }: { account: string; workspacePath: string; streaming: boolean }) {
  const { t } = useTranslation();
  const entry = useClaudeUsageStore((state) => state.accounts[account]);
  const refresh = useClaudeUsageStore((state) => state.refresh);
  const [open, setOpen] = useState(false);
  const [now, setNow] = useState(Date.now);
  const triggerRef = useRef<HTMLButtonElement>(null);
  const popoverRef = useRef<HTMLElement>(null);
  useDismissOnOutsidePress(open, () => setOpen(false), [triggerRef, popoverRef]);
  const allowOpenChange = useTriggerToggle(open, triggerRef);
  useEffect(() => { void refresh(account, workspacePath); }, [account, workspacePath, refresh]);
  // Refresh after a turn or when inspecting the card; the account cache
  // coalesces split panes and workspace switches. No interval polling.
  useEffect(() => { if (open || !streaming) void refresh(account, workspacePath); }, [open, streaming, account, workspacePath, refresh]);
  useEffect(() => {
    const resets = [entry?.data?.fiveHour?.resetsAt, entry?.data?.sevenDay?.resetsAt]
      .filter((value): value is number => typeof value === "number" && value > Date.now());
    if (!resets.length) return;
    const timer = window.setTimeout(() => {
      setNow(Date.now());
      void refresh(account, workspacePath, true);
    }, Math.min(2_147_483_647, Math.max(0, Math.min(...resets) - Date.now() + 1000)));
    return () => window.clearTimeout(timer);
  }, [entry?.data, account, workspacePath, refresh]);
  useEffect(() => {
    if (!entry?.error) return;
    const timer = window.setTimeout(() => setNow(Date.now()), Math.max(0, entry.attemptedAt + 30_001 - Date.now()));
    return () => window.clearTimeout(timer);
  }, [entry?.error, entry?.attemptedAt]);

  const currentTime = Math.max(now, Date.now());
  const retryAt = entry?.error ? entry.attemptedAt + 30_000 : null;
  const coolingDown = retryAt !== null && retryAt > currentTime;
  const fiveHour = remainingPercent(entry?.data?.fiveHour, currentTime);
  const sevenDay = remainingPercent(entry?.data?.sevenDay, currentTime);
  const percentage = (value: number | null) => value === null ? "—" : `${value}%`;
  const label = t("chat.claudeQuota");
  const summary = entry?.data ? t("chat.quotaSummary", { fiveHour: percentage(fiveHour), sevenDay: percentage(sevenDay) }) : label;
  const renderWindow = (window: ClaudeUsageWindow | null | undefined, title: string) => {
    const remaining = remainingPercent(window, currentTime);
    return <div className="space-y-1.5">
      <div className="flex items-center justify-between gap-3 text-caption-1-medium">
        <span className="text-text-secondary">{title}</span>
        <span className="text-text-primary">{remaining === null ? "—" : t("chat.quotaRemaining", { percent: remaining })}</span>
      </div>
      {remaining !== null && <div role="progressbar" aria-label={title} aria-valuemin={0} aria-valuemax={100} aria-valuenow={remaining}
        aria-valuetext={t("chat.quotaRemaining", { percent: remaining })} className="h-1.5 overflow-hidden rounded-full bg-background-tertiary-default">
        <div className={cx("h-full rounded-full", remaining <= 10 ? "bg-text-error-primary" : "bg-foreground-icon-secondary")} style={{ width: `${remaining}%` }} />
      </div>}
      <p className="text-caption-2-regular text-text-tertiary">{window?.resetsAt ? t("chat.quotaResets", { time: new Date(window.resetsAt).toLocaleString() }) : t("chat.quotaResetUnknown")}</p>
    </div>;
  };
  return <DialogTrigger isOpen={open} onOpenChange={(next) => { if (allowOpenChange(next)) setOpen(next); }}>
    <span className="flex shrink-0" title={label}>
    <AriaButton ref={triggerRef} aria-label={`${label}: ${summary}`}
      className="flex shrink-0 cursor-pointer items-center gap-1 whitespace-nowrap rounded-lg px-1 py-1 text-caption-1-regular text-text-tertiary outline-none hover:bg-background-secondary-hover hover:text-text-secondary focus-visible:ring-2 focus-visible:ring-border-focus-ring">
      <Gauge className="size-3.5" aria-hidden />
      <span className="max-md:hidden">{summary}</span>
      {entry?.error && <span className="size-1 rounded-full bg-text-error-primary" aria-hidden />}
    </AriaButton>
    </span>
    <Popover ref={popoverRef} isNonModal placement="top end" offset={8}
      className="w-72 max-w-[calc(100vw-32px)] rounded-xl border border-separator-border bg-background-primary-default p-3 shadow-dropdown">
      <Dialog aria-label={label} className="space-y-3 outline-none">
        <div className="flex items-center justify-between gap-2">
          <h3 className="text-caption-1-semibold text-text-primary">{label}{entry?.data?.plan ? ` · ${entry.data.plan}` : ""}</h3>
          <AriaButton aria-label={t("chat.quotaRefresh")} isDisabled={entry?.loading || coolingDown}
            onPress={() => { void refresh(account, workspacePath, true); }}
            className="flex size-6 cursor-pointer items-center justify-center rounded-md text-foreground-icon-secondary outline-none hover:bg-background-secondary-hover focus-visible:ring-2 focus-visible:ring-border-focus-ring disabled:opacity-50">
            <RefreshCw className={cx("size-3.5", entry?.loading && "animate-spin motion-reduce:animate-none")} aria-hidden />
          </AriaButton>
        </div>
        <p className="text-caption-2-regular text-text-tertiary">{t("chat.quotaAccountWide")}</p>
        {renderWindow(entry?.data?.fiveHour, t("chat.quotaFiveHour"))}
        {renderWindow(entry?.data?.sevenDay, t("chat.quotaSevenDay"))}
        <p role="status" className="text-caption-2-regular text-text-tertiary">{entry?.loading ? t("chat.quotaLoading") : entry?.data ? t("chat.quotaUpdated", { time: new Date(entry.data.updatedAt).toLocaleString() }) : t("chat.quotaUnavailable")}</p>
        {entry?.error && <p role="alert" className="break-words text-caption-2-regular text-text-error-primary">{t(entry.data ? "chat.quotaFailed" : "chat.quotaUnavailable")} {entry.error}{coolingDown && retryAt && ` ${t("chat.quotaRetryAt", { time: new Date(retryAt).toLocaleTimeString() })}`}</p>}
      </Dialog>
    </Popover>
  </DialogTrigger>;
}
