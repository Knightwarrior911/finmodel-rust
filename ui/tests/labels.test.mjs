import test from "node:test";
import assert from "node:assert/strict";
import {
  toolRunningLabel,
  toolDoneLabel,
  toolFailedLabel,
  toolShortName,
  humanizeToolId,
  fanoutRunningLabel,
  fanoutDoneLabel,
  agentPhaseLabel,
  confidenceLabel,
  verifyStatusLabel,
  pageStatusLabel,
  dealSufficiencyLabel,
  factKeyLabel,
  formatFactValue,
  citeChipLabel,
  sourcePublisherLabel,
  sourceCardTitle,
  sourceAvatarLetter,
  sourceRowMeta,
  approvalHead,
  approvalApproveLabel,
  approvalDenyLabel,
  approvalNewVersionLabel,
  missionMetaLine,
} from "../js/labels.mjs";

test("known tools never expose snake_case", () => {
  const running = toolRunningLabel("get_financials");
  assert.match(running, /financials/i);
  assert.doesNotMatch(running, /get_financials|Running /);
  assert.equal(toolDoneLabel("get_financials"), "Pulled the financials");
  assert.match(toolFailedLabel("web_search"), /Couldn't/);
});

test("bare detail wraps with a human verb", () => {
  assert.equal(toolRunningLabel("get_quote", "NVDA"), "Checking price for NVDA…");
  assert.equal(
    toolRunningLabel("web_search", "Look up NVDA guidance from the latest call…"),
    "Look up NVDA guidance from the latest call…",
  );
});

test("unknown tools are humanized", () => {
  const label = toolRunningLabel("fetch_custom_metric");
  assert.doesNotMatch(label, /fetch_custom_metric/);
  assert.match(label, /custom metric/i);
  assert.equal(humanizeToolId("get_foo_bar"), "foo bar");
  assert.equal(toolShortName("get_financials"), "Financials");
  assert.equal(toolShortName("weird_tool", "Custom label"), "Custom label");
});

test("fanout and phase copy stay consumer-friendly", () => {
  assert.equal(fanoutRunningLabel(3), "Looking at 3 things together…");
  assert.equal(fanoutDoneLabel(3), "Finished those 3 checks");
  assert.equal(agentPhaseLabel("planning"), "Mapping out the approach…");
  assert.equal(agentPhaseLabel("synthesizing"), "Writing this up…");
  assert.equal(agentPhaseLabel("verifying"), "Double-checking the figures…");
  assert.equal(agentPhaseLabel("executing"), null);
});

test("card copy helpers stay consumer-friendly", () => {
  assert.equal(confidenceLabel("medium"), "Fairly confident");
  assert.equal(confidenceLabel("high"), "High confidence");
  assert.equal(verifyStatusLabel("partial_unverified"), "Partly checked");
  assert.equal(verifyStatusLabel("verified"), "Figures checked");
  assert.equal(pageStatusLabel("ok"), "Ready to read");
  assert.equal(dealSufficiencyLabel(true), "Looks complete");
  assert.equal(dealSufficiencyLabel(false), "Still missing pieces");
  assert.equal(factKeyLabel("deal_value"), "Deal value");
  assert.equal(factKeyLabel("announce_date"), "Announced");
  assert.equal(formatFactValue({ amount: 10, currency: "USD" }), "10 USD");
  assert.doesNotMatch(String(formatFactValue({ nested: { a: 1 } })), /\{/);
  assert.equal(citeChipLabel({ domain: "www.reuters.com" }, "s1", 0), "1");
  assert.equal(citeChipLabel({}, "s2", 1), "2");
  assert.equal(citeChipLabel({}, "s9", null), "9");
  assert.equal(sourcePublisherLabel({ domain: "www.reuters.com" }), "reuters.com");
  assert.equal(sourceCardTitle({ title: "Q2 filing", domain: "sec.gov" }), "Q2 filing");
  assert.equal(sourceCardTitle({ domain: "sec.gov" }), "sec.gov");
  assert.equal(sourceAvatarLetter("reuters.com"), "R");
  assert.equal(sourceRowMeta("ok", "filing"), "Read · Filing");
  assert.doesNotMatch(confidenceLabel("medium"), /confidence:/i);
});


test("approval vocabulary is shared and warm", () => {
  assert.equal(approvalApproveLabel(), "Go ahead");
  assert.equal(approvalDenyLabel(), "Not this time");
  assert.equal(approvalNewVersionLabel(), "Save as a new version");
  assert.match(approvalHead("export"), /OK on this write/);
  assert.match(approvalHead(""), /need your OK/);
});

test("mission meta line stays quiet and human", () => {
  assert.equal(
    missionMetaLine({
      workflow: "Earnings review",
      planDone: 2,
      planTotal: 5,
      verify: "verified",
    }),
    "Earnings review · 2 of 5 done · Figures checked",
  );
  assert.equal(missionMetaLine({}), "");
});

import { filingFormLabel, filingItemLabel } from "../js/labels.mjs";

test("filingFormLabel names the common SEC forms", () => {
  assert.equal(filingFormLabel("10-K"), "Annual report");
  assert.equal(filingFormLabel("8-K"), "Current report");
  assert.equal(filingFormLabel("10-Q"), "Quarterly report");
  // Unknown forms fall back to the code — never blank.
  assert.equal(filingFormLabel("SC 13D"), "SC 13D");
});

test("filingItemLabel is form-aware and survives sub-items", () => {
  assert.equal(filingItemLabel("8-K", "2"), "Item 2 · Financial information");
  // 8-K sub-items resolve on the major number.
  assert.equal(
    filingItemLabel("8-K", "2.02"),
    "Item 2.02 · Financial information",
  );
  assert.equal(filingItemLabel("10-K", "1A"), "Item 1A · Risk factors");
  // Same number, different form, different meaning.
  assert.equal(filingItemLabel("10-K", "2"), "Item 2 · Properties");
  // Unknown items keep the plain form.
  assert.equal(filingItemLabel("10-K", "42"), "Item 42");
});

import { scheduleDueLabel } from "../js/labels.mjs";

test("scheduleDueLabel speaks plainly and is honest about approximation", () => {
  assert.equal(scheduleDueLabel("tomorrow"), "tomorrow");
  assert.equal(scheduleDueLabel("next_week"), "in a week");
  assert.match(scheduleDueLabel("after_next_earnings"), /about five weeks/);
  assert.match(scheduleDueLabel(null), /about a week/);
});

test("memo kinds and drafting activity speak plainly", () => {
  assert.equal(memoKindLabel("earnings_note"), "Earnings note");
  assert.equal(memoKindLabel("deal_summary"), "Deal summary");
  assert.equal(memoKindLabel("whatever"), "Memo");
  assert.equal(toolRunningLabel("draft_memo"), "Writing it up…");
  assert.equal(toolDoneLabel("draft_memo"), "Drafted the memo");
});
import { memoKindLabel } from "../js/labels.mjs";
