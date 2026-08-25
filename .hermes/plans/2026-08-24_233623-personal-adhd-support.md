# Personal ADHD Support Workflow Implementation Plan

> For Hermes: Use subagent-driven-development skill to implement this plan task-by-task.

Goal: Build VIC’s personal support loop as a safe, voice-first external working memory: capture messy thoughts, turn them into reviewable suggestions, help James choose one doable next action, support short focus sessions, and recover gently after interruptions.

Architecture: Start with a narrow vertical slice rather than a broad autonomous assistant. Fieldy and voice/API inputs enter a temporary, owner-scoped capture inbox. A bounded extraction step produces reviewable proposals only; approval is explicit before anything becomes a task, appointment, memory, or reminder. The existing focus-session machinery then executes the approved next action, while a daily reset and interruption-recovery API provide the human workflow around it.

Tech Stack: Rust workspace, voiceos-core ConversationStore, SQLite migrations, Axum gateway APIs, existing focus sessions, Fieldy webhook intake, integration tests.

---

## Product behavior and safety contract

1. Capture is frictionless: “VIC, capture this…” and Fieldy transcripts are stored verbatim with source metadata and retention expiry.
2. Capture is not commitment: raw intake never automatically creates tasks, memories, schedules, messages, or outbound actions.
3. Extraction is bounded: VIC may suggest only task, appointment, worry, idea, or note candidates, with source links and a short rationale. Every candidate is owner-scoped, expires, and remains pending review.
4. Approval is explicit and item-level. Reject, edit, approve, snooze, and discard are separate operations and are audited.
5. Daily support shows at most three priorities, one recommended first action, estimated effort, and a five-minute fallback. It must not shame or imply failure.
6. Interruption recovery preserves the exact restart action and asks only what is needed to resume.
7. Proactive behavior is opt-in and quiet-hours aware. The first release should not send unsolicited notifications; it should expose a reviewable “what needs attention?” result.

## Current foundation

- `services/voiceos-core/src/fieldy.rs` already verifies signed Fieldy events and stores temporary transcript intake.
- `services/voiceos-core/src/focus.rs` already supports snapshots, five-minute/low-energy/restart modes, interruption, resume, completion, and audit events.
- `services/voiceos-core/src/schema.rs` already contains Fieldy intake, task, goal, project, and focus-session tables.
- `services/voiceos-gateway-rs/src/api/focus.rs` already exposes capture and focus-related routes.
- Existing tests: `services/voiceos-core/tests/fieldy_webhook.rs` and `focus_support.rs`.
- Proactive proposal code exists but has undergone repeated safety review; re-read it before extending it and preserve its owner, expiry, evidence, and fail-closed constraints.

## Implementation sequence

### Task 1: Define the personal workflow records and states

Files:
- Modify: `services/voiceos-core/src/schema.rs`
- Modify: `services/voiceos-core/src/model.rs`
- Modify: `services/voiceos-core/src/lib.rs`
- Test: `services/voiceos-core/tests/personal_support.rs`

Add typed records for `PersonalCapture`, `CaptureProposal`, `DailyFocusReset`, and review decisions. Use explicit states such as `received`, `reviewing`, `approved`, `rejected`, `snoozed`, `discarded`, and `expired`. Include owner_id, source, source_id, raw/structured content, created_at, expires_at, and audit linkage. Add indexes by owner, status, and created_at. Reject malformed timestamps, empty text, cross-owner IDs, and expired writes.

TDD: add failing migration/model tests first; run the focused test; implement the smallest schema and typed accessors; rerun focused tests and `cargo test -p voiceos-core`.

### Task 2: Make capture a real temporary inbox

Files:
- Modify: `services/voiceos-core/src/fieldy.rs`
- Create or modify: `services/voiceos-core/src/personal_support.rs`
- Modify: `services/voiceos-core/src/lib.rs`
- Test: `services/voiceos-core/tests/personal_support.rs`

Implement `capture_personal_input(owner_id, source, text, occurred_at, retention)` and owner-scoped inbox listing. Deduplicate Fieldy events and voice capture IDs. Preserve the exact transcript separately from normalized display text. Add approve/reject/discard operations that only change the intake record and append an audit event; none may create a task or memory by themselves.

Acceptance tests: duplicate capture is idempotent; another owner cannot read or mutate it; expired capture is hidden; raw transcript is preserved; capture produces no task, memory, schedule, delivery, or notification record.

### Task 3: Add bounded ADHD-oriented extraction

Files:
- Modify: `services/voiceos-core/src/personal_support.rs`
- Modify: `services/voiceos-core/src/model.rs`
- Test: `services/voiceos-core/tests/personal_support.rs`

Define a strict extraction contract with finite categories: `task`, `appointment`, `worry`, `idea`, `note`. Require source capture ID, confidence, concise title, optional details, suggested next action, and candidate expiry. Reject unknown JSON fields, hidden action language, outbound instructions, destructive instructions, arbitrary URLs, cross-owner evidence, expired candidates, and output that attempts to create or mutate anything directly.

The extractor must be injectable so tests can use deterministic structured output. Keep provider/network invocation outside the core store. Add regression tests for messy brain dumps, multiple candidates, malformed output, hidden instructions, and empty/no-candidate results.

### Task 4: Add explicit review and approval conversion

Files:
- Modify: `services/voiceos-core/src/personal_support.rs`
- Modify: `services/voiceos-core/src/task_initiative.rs` if task kickoff is reused
- Test: `services/voiceos-core/tests/personal_support.rs`

Implement proposal listing and item-level decisions. Approval must require the owner and a current, unexpired proposal. Convert only the approved category through explicit typed methods: task approval creates a task in `proposed` or `ready` according to the user’s choice; note/idea/worry remain in their designated store; appointment approval must remain a reviewable event until a calendar integration is explicitly authorized. Every conversion stores source proposal IDs and audit events. Rejection and discard must never partially convert.

### Task 5: Build the “VIC, help me get unstuck” flow

Files:
- Modify: `services/voiceos-core/src/focus.rs`
- Modify: `services/voiceos-core/src/model.rs`
- Test: `services/voiceos-core/tests/personal_support.rs`, `focus_support.rs`

Add a personal focus reset that returns: current interruption state, up to three priorities, one recommended task, the first physical next action, a five-minute version, and a single optional question when ambiguity blocks action. Reuse `focus_snapshot` and existing restart semantics; do not create a new parallel focus engine. Ensure the result remains useful when there are no tasks: offer capture or a tiny restart action rather than inventing work.

Acceptance tests cover normal, low-energy, restart-after-interruption, empty-board, and more-than-three-task cases.

### Task 6: Expose the workflow through the gateway

Files:
- Modify: `services/voiceos-gateway-rs/src/api/mod.rs`
- Modify: `services/voiceos-gateway-rs/src/api/focus.rs`
- Create or modify: gateway API tests

Add authenticated owner-scoped routes for capture, inbox listing, proposal listing, proposal decision, daily reset, start five-minute session, interrupt, resume, and complete. Return stable typed JSON. Keep all mutations auditable and require the existing actor/device context. Do not add outbound notification routes in this slice.

Run gateway focused tests, then workspace tests. Verify unauthorized and cross-owner requests return safe errors without leaking record existence.

### Task 7: Add a minimal voice interaction contract

Files:
- Modify: `services/voiceos-ontology/src/catalog.rs`
- Modify: `services/voiceos-ontology/src/resolver.rs`
- Modify: `services/voiceos-ontology/tests/ontology.rs`
- Modify: gateway intent dispatch if required

Support natural utterances equivalent to: “capture this,” “what should I do next,” “help me get unstuck,” “I’m interrupted,” “show my captures,” “review that,” and “discard that.” Parsing must not turn arbitrary conversation into a capture. Require explicit capture intent or a direct response to a capture prompt. Add tests for false positives and owner/device propagation.

### Task 8: Dogfood with James before proactive automation

Use the system manually for five real interactions:
1. One unfiltered brain dump.
2. One “help me get unstuck” request.
3. One five-minute focus session.
4. One interruption and restart.
5. One review/rejection of a Fieldy-derived proposal.

Record only observed friction and requested behavior. Do not infer permanent ADHD traits from these sessions. Tune wording, limits, retention, and interruption policy from explicit feedback. Only after this works should we design quiet hours, gentle follow-up, and optional proactive check-ins.

## Verification checklist

- `cargo fmt --all -- --check`
- `cargo test -p voiceos-core`
- `cargo test -p voiceos-gateway-rs`
- `cargo test -p voiceos-ontology`
- `cargo test --workspace`
- `git diff --check`
- Direct API tests prove no capture or extraction path creates outbound delivery, notification, task, memory, or calendar side effects without explicit approval.
- Cross-owner, expired, malformed, duplicate, and hidden-instruction cases all fail closed.
- Manual dogfood confirms the first response is concrete and small, not a long planning lecture.

## Risks and decisions to preserve

- Do not begin with autonomous reminders; false positives and interruption cost are high.
- Do not store inferred attention patterns as durable memory without explicit review.
- Do not let an LLM-generated proposal bypass typed validation, owner checks, evidence resolution, or expiry checks.
- Keep raw capture retention finite and configurable.
- The first milestone is a reliable personal loop, not a complete life-management system.
