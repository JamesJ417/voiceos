# VIC Proactive Companion Roadmap

Status: design/scaffold only. This document does not enable outreach, schedules, notifications, research monitors, or automatic task/memory creation.

## Goal

Build a private, approval-aware companion layer that gives VIC good reasons to initiate occasional conversations with James about active projects, pending decisions, useful discoveries, and friendly check-ins—without becoming a noisy notification system or acting without permission.

## Product principles

1. VIC should have a reason to speak. Every proposed outreach must explain why it matters now, what decision or question it creates, or what useful next step it can prepare.
2. Suggestions are not actions. VIC may observe approved context, prepare research, draft questions, and propose next steps. Sending outreach, creating tasks, changing settings, spending money, and saving durable memories remain approval-gated.
3. The Rust VoiceOS control plane owns identity, policy, schedules, approvals, persistence, audit, and delivery state. Models may classify, summarize, and draft, but they do not own authority.
4. Context is scoped and attributable. A proposal must identify its project, source evidence, owner, confidence, and expiration. Uncertain or cross-scope claims fail closed.
5. Attention is scarce. Quiet hours, rate limits, snooze, topic muting, urgency classes, and a visible pending queue are required before live outreach.
6. Friendly does not mean pretending. VIC can use a warm voice, but must distinguish facts, synthesis, uncertainty, and speculation.

## Desired experience

Examples of valid proactive proposals:

- Project check-in: “The Fieldy intake foundation is complete. The next useful slice is the review endpoint. Want to work on that today?”
- Clarifying question: “The webhook design still needs an owner identity decision. Should the first version support only you?”
- Stagnation signal: “The Fieldy connection has been waiting on device setup. I prepared a local signed-payload test so we can continue without the device.”
- Relevant discovery: “I found an AI tool that may help SMB Sentinel create restaurant training material. Want the short version?”
- Interruption recovery: “You were deciding how transcript proposals should become tasks. The unresolved choice was approval timing.”

## Architecture

The proactive layer is a proposal pipeline:

1. Context sources expose only approved, owner-scoped facts and events.
2. Deterministic monitors detect conditions such as stale work, unanswered questions, deadlines, or explicit research subscriptions.
3. A bounded reasoning step ranks candidate reasons to speak and drafts a concise message.
4. Policy evaluates quiet hours, frequency, topic preferences, sensitivity, urgency, and required approval.
5. A durable outreach proposal is queued with evidence and an expiration time.
6. James reviews, dismisses, snoozes, answers, or approves delivery.
7. Only an approved delivery adapter contacts a configured channel.
8. The response is linked back to the originating proposal and conversation scope.

No monitor should read arbitrary private data by default. Research monitors must use explicitly selected topics and retain source, retrieval time, content hash, and citations.

## Backend work packages

### 1. Canonical proactive records

Add Rust-owned records for:

- `proactive_subscription`: topic, project scope, source type, cadence, quiet hours, status, and owner;
- `proactive_candidate`: detected reason, evidence references, priority, confidence, expiration, and deduplication key;
- `outreach_proposal`: drafted message, channel, approval state, risk class, source candidate, and delivery deadline;
- `outreach_delivery`: approved attempt, provider/channel result, idempotency key, timestamps, and response linkage;
- `proactive_feedback`: useful, not useful, mute topic, snooze, correction, or answer action.

All records require owner scope, provenance, audit events, and idempotency behavior.

### 2. Candidate detection

Start with deterministic, local signals:

- task or project has had no meaningful progress for a configurable interval;
- a task or design decision is explicitly blocked on a question;
- a scheduled deadline is approaching;
- a user-approved monitor has new evidence;
- an interruption has a stored restart point;
- a completed workflow suggests a safe next step.

Do not begin with unrestricted “think about anything” background analysis. It is expensive, difficult to govern, and likely to create noise.

### 3. Proposal drafting and ranking

Create a bounded reasoning contract that receives only candidate evidence and approved context. It must return structured output:

- reason category;
- one-sentence rationale;
- proposed message;
- related project/task/topic;
- confidence;
- urgency and interruption cost;
- required approval level;
- evidence IDs;
- expiration time.

Reject outputs with missing evidence, hidden actions, unsupported urgency, durable-memory mutations, or messages that expose unrelated context.

### 4. Attention policy

Implement policy before live delivery:

- owner-configured quiet hours;
- daily and per-topic outreach budgets;
- urgent versus normal versus interesting priority;
- cooldown after dismissal;
- snooze until time or event;
- “stop asking about this” topic controls;
- duplicate suppression;
- sensitivity and channel restrictions;
- fail-closed behavior when delivery permission is absent.

### 5. Review and conversation continuity

Add owner-authenticated APIs for:

- listing pending proposals;
- viewing evidence and draft text;
- approving, editing, dismissing, or snoozing;
- answering a question;
- muting a topic;
- following the proposal into the originating conversation.

A reply must retain proposal ID, project scope, and conversation scope. An answer may update a blocked workflow only through the normal approval and task policy paths.

### 6. Delivery adapters

Begin with an internal queue and the existing VoiceOS surfaces. Add external channels only after policy and audit are proven. Each adapter must support explicit approval, idempotency, retry limits, delivery receipts, revocation, and no-secret logging.

Potential later channels:

- Android notification or conversation surface;
- kiosk/panel card;
- configured AgentMail inbox;
- other explicitly approved channels.

### 7. Research monitors

Research is opt-in and separate from project/task monitoring. A monitor definition must include topic, allowed sources, cadence, maximum results, channel, and retention policy. Retrieved pages are untrusted data and cannot issue instructions. Store citations and hashes; deduplicate by canonical URL/content hash; surface only material, relevant findings.

### 8. Privacy, audit, and operations

Record candidate creation, ranking, proposal, approval, edit, dismissal, snooze, mute, delivery, failure, response, and deletion. Do not log full private transcripts or message bodies in ordinary operational logs. Provide export, discard, retention expiry, and monitor disable controls.

## Build order

1. Write the contract and policy fixtures for one internal project check-in.
2. Add canonical proactive/proposal records and migration.
3. Add a deterministic stagnation detector over existing project/task/event records.
4. Add proposal creation with evidence and deduplication.
5. Add list/detail/approve/dismiss/snooze/mute endpoints.
6. Add a dry-run queue that never contacts James and exposes proposals to the existing UI.
7. Add structured drafting behind a bounded capability and validate its output.
8. Add quiet hours, budgets, cooldowns, expiration, and audit coverage.
9. Add an internal VoiceOS delivery surface with verified read-back.
10. Add one opt-in research monitor with citations and an end-to-end test.
11. Conduct a privacy and interruption-quality review.
12. Enable live outreach only after explicit approval of channel, schedule, limits, and rollback behavior.

## First implementation slice

The smallest useful vertical slice is “stale project → reviewable proposal,” with no live outreach:

- input: an owner-scoped project with no progress event for a configured interval;
- detector: deterministic and idempotent;
- output: one `outreach_proposal` containing project ID, evidence event IDs, rationale, draft question, confidence, expiration, and `pending_review` state;
- API: list and inspect the proposal, then dismiss or approve it for dry-run display only;
- tests: owner isolation, duplicate suppression, expiration, quiet hours, missing evidence, and no task/memory mutation.

This slice proves the authority and review loop before adding model reasoning or external channels.

## Acceptance tests

- A stale project creates at most one active candidate for its deduplication window.
- A candidate without valid owner-scoped evidence is rejected.
- A proposal never creates a task, memory, schedule, or outbound message by itself.
- Quiet hours and topic mute prevent delivery and are visible in audit state.
- Approval is required before any delivery attempt.
- Editing a draft preserves the original generated text and records the editor/action.
- Dismissal and snooze suppress the same proposal according to policy.
- A reply resolves to the correct project and conversation scope.
- Duplicate jobs and retries do not duplicate proposals or deliveries.
- Delivery failures are visible, bounded, and retryable without expanding permissions.
- Research results retain citations and cannot execute instructions from retrieved content.
- Owner A cannot see, approve, or receive Owner B’s proposals.

## Open decisions for our walkthrough

1. Should the first live channel be the Android app, the kiosk, AgentMail, or a combination?
2. What default outreach budget feels friendly rather than noisy—for example, one normal check-in per day plus urgent exceptions?
3. What quiet hours should be the initial default?
4. Which projects and topics are explicitly approved for the first monitor?
5. Should every normal proposal require approval, or may James pre-approve a narrow class such as internal project check-ins?
6. How long should transcript-derived evidence remain available in the review buffer?
7. What should count as “stagnant”: elapsed time, no event, no task status change, or a combination?

## Safety boundary

This roadmap does not enable a background scheduler, add credentials, contact a channel, create durable memories, or change gateway configuration. Those are separate implementation and approval steps after the dry-run slice passes its tests.
