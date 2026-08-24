import assert from "node:assert/strict";
import test from "node:test";
import { approvalDecisionFromText } from "../app/approval-intent.js";

test("recognizes an explicit approval phrase", () => {
  assert.equal(approvalDecisionFromText("  Yes, approve. "), "approve");
});

test("recognizes an explicit denial phrase", () => {
  assert.equal(approvalDecisionFromText("cancel"), "deny");
});

test("does not treat ordinary conversation as an approval decision", () => {
  assert.equal(approvalDecisionFromText("please explain the test result"), null);
});
