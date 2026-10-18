import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import i18n from "@/lib/i18n";
import { HistoryCacheButton } from "./HistoryCacheButton";

Object.assign(globalThis, { IS_REACT_ACT_ENVIRONMENT: true });
let container: HTMLDivElement;
let root: Root;
beforeEach(() => { container = document.createElement("div"); document.body.append(container); root = createRoot(container); });
afterEach(async () => { await act(async () => root.unmount()); container.remove(); });

it("shows updating cached history and prevents duplicate refresh clicks", async () => {
  const refresh = vi.fn();
  await act(async () => root.render(<HistoryCacheButton cache={{ cachedAt: 1000, refreshing: true, refreshError: null }} onRefresh={refresh} />));
  expect(container.querySelector('[role="status"]')?.textContent).toContain(i18n.t("chat.historyCacheUpdating"));
  const button = container.querySelector("button")!;
  expect(button.disabled).toBe(true);
  await act(async () => button.click());
  expect(refresh).not.toHaveBeenCalled();
});

it("keeps failure details in the icon description and offers a retry", async () => {
  const refresh = vi.fn();
  await act(async () => root.render(<HistoryCacheButton cache={{ cachedAt: 1000, refreshing: false, refreshError: "Remote command timed out (300s)" }} onRefresh={refresh} />));
  expect(container.querySelector('[role="status"]')?.textContent).toContain(i18n.t("chat.historyCacheUpdateFailed"));
  expect(container.querySelector("button")?.title).toContain("Remote command timed out (300s)");
  expect(container.querySelector("button")?.textContent).toBe("");
  expect(container.querySelector('[role="status"]')?.className).toContain("sr-only");
  await act(async () => container.querySelector("button")!.click());
  expect(refresh).toHaveBeenCalledOnce();
});

it("identifies a successfully refreshed local snapshot", async () => {
  await act(async () => root.render(<HistoryCacheButton cache={{ cachedAt: 1000, refreshing: false, refreshError: null, updated: true }} onRefresh={() => {}} />));
  expect(container.querySelector('[role="status"]')?.textContent).toContain(i18n.t("chat.historyCacheUpdated"));
  expect(container.querySelector('[role="alert"]')).toBeNull();
});
