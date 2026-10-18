import { useEffect, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import ShieldCheck from "lucide-react/dist/esm/icons/shield-check";
import { Button } from "@/components/base/buttons/button";
import type { Message } from "@/lib/ipc";
import { TOOL_PERMISSION_ANSWER_KEY, type ToolPermissionDecision } from "@/lib/tool-permission";
import { useChatStore } from "../store";
import { useScopedSessionKey } from "../split/session-scope";

/** Native approval actions have their own controls, separate from the
 * model's AskUserQuestion form. Input is text-only JSON for human review. */
export function ToolPermissionCard({ message }: { message: Message }) {
  const { t } = useTranslation();
  const key = useScopedSessionKey();
  const respond = useChatStore((state) => state.respondToQuestion);
  const [submitting, setSubmitting] = useState(false);
  const inFlight = useRef(false);
  const mounted = useRef(true);
  useEffect(() => { mounted.current = true; return () => { mounted.current = false; }; }, []);
  const question = message.question;
  const permission = question?.toolPermission;
  if (!permission || question?.status !== "pending") return null;
  const decide = async (decision: ToolPermissionDecision | null) => {
    if (!key || inFlight.current) return;
    inFlight.current = true;
    setSubmitting(true);
    try {
      await respond(key, message.seq, decision === null ? null : { [TOOL_PERMISSION_ANSWER_KEY]: decision });
    } finally {
      inFlight.current = false;
      if (mounted.current) setSubmitting(false);
    }
  };
  return (
    <section className="flex w-full flex-col gap-2.5 text-left" aria-label={t("chat.toolPermissionTitle")}>
      <div className="flex items-center gap-2 text-caption-1-medium text-text-primary">
        <ShieldCheck className="size-4 shrink-0" aria-hidden />
        <span>{t("chat.toolPermissionTitle")}</span>
        <span className="rounded-md bg-background-tertiary-default px-1.5 py-0.5 font-mono text-caption-1-regular">{permission.toolName}</span>
      </div>
      <p className="text-caption-1-regular text-text-secondary">{t("chat.toolPermissionDescription")}</p>
      <pre aria-label={t("chat.toolPermissionInput")} className="max-h-52 overflow-auto whitespace-pre-wrap break-all rounded-lg border border-border-secondary bg-background-tertiary-default p-2.5 font-mono text-caption-1-regular text-text-primary">{permission.inputJson}</pre>
      <div className="flex flex-wrap items-center justify-end gap-2">
        <Button size="small" variant="ghost" disabled={submitting || !key} onClick={() => void decide(null)}>{t("chat.toolPermissionSkip")}</Button>
        <Button size="small" variant="secondary" disabled={submitting || !key} onClick={() => void decide("deny")}>{t("chat.toolPermissionDeny")}</Button>
        <Button size="small" disabled={submitting || !key} onClick={() => void decide("allow_once")}>{t("chat.toolPermissionAllowOnce")}</Button>
      </div>
      {submitting && <p role="status" className="text-caption-1-regular text-text-secondary">{t("chat.toolPermissionSubmitting")}</p>}
    </section>
  );
}

/** Preserve the reviewed input and the explicit one-call decision in the
 * timeline; settled/expired records have no approval controls. */
export function ToolPermissionRecord({ message }: { message: Message }) {
  const { t } = useTranslation();
  const question = message.question;
  const permission = question?.toolPermission;
  if (!question || !permission) return null;
  const statusKey = question.status === "pending" ? "toolPermissionWaiting"
    : question.status === "cancelled" ? "toolPermissionExpired"
    : question.status === "dismissed" ? "toolPermissionSkipped"
    : question.answers?.[TOOL_PERMISSION_ANSWER_KEY] === "allow_once" ? "toolPermissionAllowed"
    : "toolPermissionDenied";
  return (
    <div className="flex max-w-[85%] flex-col gap-1 rounded-xl border border-border-secondary bg-background-secondary-default px-3.5 py-2.5 text-left">
      <div className="flex items-center gap-1.5 text-caption-1-medium text-text-secondary">
        <ShieldCheck className="size-3.5 shrink-0" aria-hidden />
        <span>{t("chat.toolPermissionTitle")} · {permission.toolName}</span>
      </div>
      <p className="text-caption-1-regular text-text-secondary">{t(`chat.${statusKey}`)}</p>
      <pre aria-label={t("chat.toolPermissionInput")} className="max-h-32 overflow-auto whitespace-pre-wrap break-all font-mono text-caption-1-regular text-text-secondary">{permission.inputJson}</pre>
    </div>
  );
}
