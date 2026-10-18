/** Native protocol values stay stable across UI languages. The backend
 * validates this exact single-key answer against its parked tool context. */
export const TOOL_PERMISSION_ANSWER_KEY = "__ccgui_tool_permission";
export type ToolPermissionDecision = "allow_once" | "deny";

export function isToolPermissionAnswer(answers: Record<string, string | string[]> | null): boolean {
  if (answers === null) return true;
  return Object.keys(answers).length === 1 &&
    (answers[TOOL_PERMISSION_ANSWER_KEY] === "allow_once" || answers[TOOL_PERMISSION_ANSWER_KEY] === "deny");
}
