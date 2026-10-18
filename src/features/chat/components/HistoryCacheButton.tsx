import { useId } from "react";
import { useTranslation } from "react-i18next";
import RefreshCw from "lucide-react/dist/esm/icons/refresh-cw";
import { cx } from "@/utils/cx";
import { PANEL_TOGGLE_CLASSES } from "../panel-toggle-classes";
import type { SessionState } from "../store/stream";

export function HistoryCacheButton({ cache, onRefresh }: {
  cache: SessionState["historyCache"];
  onRefresh: () => void;
}) {
  const { t } = useTranslation();
  const descriptionId = useId();
  if (!cache) return null;
  const label = t("chat.historyCacheRefresh");
  const status = t(cache.refreshing ? "chat.historyCacheUpdating" : cache.refreshError ? "chat.historyCacheUpdateFailed" : cache.updated ? "chat.historyCacheUpdated" : "chat.historyCacheLocal");
  const description = [status, t("chat.historyCacheTime", { time: new Date(cache.cachedAt).toLocaleString() }), cache.refreshError].filter(Boolean).join("\n");
  return (
    <span className="flex shrink-0" title={`${label}\n${description}`}>
      <button
        type="button"
        title={`${label}\n${description}`}
        aria-label={label}
        aria-describedby={descriptionId}
        aria-busy={cache.refreshing}
        disabled={cache.refreshing}
        onClick={onRefresh}
        className={cx(PANEL_TOGGLE_CLASSES, "relative disabled:cursor-default disabled:opacity-50")}
      >
        <RefreshCw className={cx("size-4", cache.refreshing && "animate-spin motion-reduce:animate-none")} aria-hidden />
        {cache.refreshError && !cache.refreshing && <span className="absolute right-1 top-1 size-1.5 rounded-full bg-text-error-primary" aria-hidden />}
      </button>
      <span id={descriptionId} role="status" className="sr-only">{description}</span>
    </span>
  );
}
