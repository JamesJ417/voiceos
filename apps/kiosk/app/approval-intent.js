const approvals = new Set(["approve", "approved", "yes", "yes approve", "confirm"]);
const denials = new Set(["deny", "denied", "no", "no deny", "reject", "cancel"]);

export function approvalDecisionFromText(text) {
  const normalized = String(text).toLowerCase().replace(/[^a-z ]/g, " ").replace(/\s+/g, " ").trim();
  if (approvals.has(normalized)) return "approve";
  if (denials.has(normalized)) return "deny";
  return null;
}
