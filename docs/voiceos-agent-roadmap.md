# VoiceOS agent operating system roadmap

## Product direction

VoiceOS should become a private, owner-controlled agent operating environment:

- one continuous identity, conversation, and memory across phone and wall terminals;
- voice-first interaction with touch used for choosing, approving, arranging, and reviewing;
- a Rust control plane that owns identity, policy, memory, tasks, schedules, audit, and capability permissions;
- replaceable local and cloud models that reason but do not own state or authority;
- visible work: every plan, tool call, scheduled job, token, approval, and result can be inspected;
- gradual self-improvement through reviewed skills and automations, never unrestricted self-modifying code.

## Architecture decision

Keep the current VoiceOS architecture and extend it. Do not install another agent framework as the operating system and do not translate an entire fast-moving Python agent codebase to Rust.

The Rust layer remains the authority. Python, MCP servers, local model runtimes, browser workers, and creative applications are replaceable capability adapters. The Android app and touchscreen kiosk remain presentation and approval surfaces.

Hermes Agent is the most relevant design reference for procedural skills, post-task review, skill-write approval, and scheduled jobs. We should study and reproduce the small number of patterns that fit VoiceOS rather than inherit its complete runtime. Its official documentation describes agent-managed skills, background self-improvement review, approval gates, and cron integration. See [Hermes skills](https://hermes-agent.nousresearch.com/docs/user-guide/features/skills) and [Hermes cron](https://github.com/NousResearch/hermes-agent/blob/main/website/docs/user-guide/features/cron.md).

## Priority sequence

### Phase 0 — Interaction reliability

Status: implemented; final hands-on reliability gate remains.

1. Copy an exact VoiceOS response to the Android clipboard.
2. Make Repeat reliably replay the last response through Android text-to-speech.
3. Make Command, History, and System real application destinations.
4. Add a Test Voice control and visible speech-engine status.
5. Preserve the last displayed response across Android activity recreation.

Exit gate: ten consecutive question, answer, playback, repeat, copy, and navigation tests on the Pixel.

### Phase 1 — Trusted agent kernel

Build these before autonomous skills, whole-home control, or multi-agent execution.

1. Create a versioned master VoiceOS charter shared by all providers. Separate stable identity and safety policy from task-specific prompts.
2. Add canonical Rust records for goals, projects, tasks, steps, jobs, skills, automation proposals, execution attempts, and artifacts.
3. Add an append-only event stream so the UI can show what the agent is doing in real time.
4. Promote existing token fields into telemetry: provider, model, input/output tokens, tokens per second, latency, GPU memory, cost, and outcome.
5. Implement capability leases: an agent receives only the typed tools required for one approved job and only for a bounded time.
6. Add checkpoints, cancellation, idempotency, and rollback metadata to every mutating workflow.

Exit gate: an interrupted or failed job can resume or roll back without losing auditability or expanding permissions.

### Phase 2 — Reviewed self-improvement and automation

“Recursive self-improvement” will be implemented as a controlled proposal loop:

1. Observe a completed workflow and its errors, corrections, tool sequence, and result.
2. Detect a reusable procedure only after evidence such as repeated success, a non-trivial recovery, or an explicit user correction.
3. Draft a portable `SKILL.md`, tests, required capabilities, and a risk classification.
4. Run the skill in a restricted test environment against recorded cases.
5. Present a diff and evidence to the user for approval.
6. Version the approved skill and retain one-click rollback.
7. After repeated successful manual use, propose—not automatically enable—a schedule or event trigger.

Code, system prompts, validators, and permission policy cannot be silently rewritten. High-impact skills always require approval, and turning a skill into an automation requires a separate approval.

Exit gate: the system can propose, test, approve, version, run, disable, and roll back one low-risk skill and one scheduled automation.

### Phase 3 — ADHD-aware execution system

1. Detect goals or tasks likely to exceed twenty minutes.
2. Offer the next five concrete steps, each sized for approximately one focused work block.
3. Ask only the minimum questions needed to choose the first step.
4. Do preparatory research and setup that fits the approved capability boundary.
5. Provide start, pause, resume, finish, and “what is the next physical action?” controls.
6. Add Pomodoro timers, contextual reminders, consistency streaks without shame mechanics, and end-of-block review.
7. Put tasks, calendar, reminders, and scheduled jobs on both phone and wall displays.

The first implementation should be a Rust task schema plus a deterministic task-breakdown validator. Model-generated steps must have a verb, an observable outcome, an estimated duration, and no hidden destructive action.

### Phase 4 — Proactive learning and mentor library

1. Schedule three optional questions at a time, up to four windows per day.
2. Maintain a question budget, quiet hours, snooze, and “stop asking about this” controls.
3. Select questions based on missing information that would materially improve current goals—not curiosity alone.
4. Store answers as attributed, correctable memories with confidence and retention policy.
5. Propose research topics from the user’s interests; browsing and ingestion remain separately approved.

For the mentor agent, ingest user-provided notes, owned books where personal indexing is permitted, authorized transcripts, and public sources. Store source metadata and citations. The system should synthesize and compare ideas rather than impersonate a living person or present generated advice as that person’s words. Each answer should distinguish source-grounded teaching, VoiceOS synthesis, and uncertainty.

### Phase 5 — Hybrid memory and Obsidian projection

The existing Rust SQLite memory core remains authoritative. Extend it with:

- structured facts, preferences, people, projects, commitments, and provenance;
- full-text search for exact language;
- embeddings for semantic retrieval;
- temporal and graph relationships;
- contradiction detection, confidence, correction, expiration, and deletion;
- hot recent context, rolling summaries, durable facts, documents, and procedural skills as separate memory classes.

Mem0 is a useful benchmark and design reference for user/session/agent memory and evaluation, but adopting its Python runtime as the source of truth would work against the Rust-backbone goal. See the [Mem0 repository](https://github.com/mem0ai/mem0) and [research paper](https://arxiv.org/abs/2504.19413).

Treat an Obsidian vault as a human-readable projection of selected VoiceOS knowledge, not the primary database. A one-way exporter should produce Markdown notes, links, tags, task summaries, and source citations. Later, reviewed changes can be imported through validation. VoiceOS must continue to work if Obsidian is closed or removed.

### Phase 6 — Whole-home agent surfaces

1. Replace device-owned continuity with owner-owned continuity and device-specific presence.
2. Enroll each room terminal with a role, microphone, speaker, display, and revocable credential.
3. Synchronize current conversation, cards, approvals, task state, timers, and notifications through the Rust event stream.
4. Add room-aware audio routing and visible privacy state.
5. Add wake-word support only after local mute, hardware indication, retention controls, and false-activation testing.
6. Make every screen able to render choice grids, checklists, calendars, comparisons, images, progress, and approvals.
7. Keep model inference on the GPU rig; run the Rust authority on the HP only after database replication and recovery tests pass.

### Phase 7 — Safe live applications and creative tools

When a user asks for a widget, site, visualization, or game, VoiceOS should create an artifact in an isolated workspace, build it in a restricted runner, assign it a stable local URL, scan the result, and show it in a sandboxed kiosk view. Generated applications receive no host credentials and no privileged loopback access.

OpenCut is strategically aligned because its announced rewrite includes a Rust core, Editor API, plugins, MCP, headless mode, and scripting. The rewrite is still under active development, so begin with an adapter contract and a disposable evaluation deployment rather than making it a core dependency. See the [OpenCut repository](https://github.com/opencut-app/opencut).

### Phase 8 — Web research and multi-model teams

Firecrawl can become an optional web-ingestion worker because it converts web content into agent-ready structured material. It must run outside the trusted control plane: fetched pages are untrusted data, cannot issue tool instructions, and must retain URL, retrieval time, content hash, and citations. See the [Firecrawl repository](https://github.com/firecrawl/firecrawl).

Do not combine CrewAI, AutoGen, and another orchestration framework inside the production control plane. That would create competing state, retry, memory, and permission systems. AutoGen is now in maintenance mode and directs new projects to Microsoft Agent Framework, so it should be studied only for patterns. See [AutoGen](https://github.com/microsoft/autogen). CrewAI may be used in a disposable evaluation harness for role-based teams; it should not own VoiceOS memory or tools. See [CrewAI](https://github.com/crewAIInc/crewAI).

Implement the desired three-model collaboration natively:

- fast local model: classify, retrieve, and draft;
- deep local model: reason, critique, and plan;
- Codex bridge: difficult implementation or explicit high-confidence verification;
- deterministic Rust coordinator: choose roles, enforce budgets, merge evidence, and stop loops.

Parallel model use should be reserved for jobs where independent answers materially improve confidence. Normal conversation remains a single fast-model turn.

## Next three implementation slices

### Slice A — Pixel response actions and audio reliability

Add Copy, repair Repeat/TTS initialization, expose audio diagnostics, finish live History/System navigation, build, install, and run a ten-turn test.

Implementation status (August 2, 2026): the updated debug build is installed on
the Pixel. Copy, Repeat, selectable transcript/history text, live History and
System destinations, speech-engine status, speech-speed control, and installed
voice selection are implemented. The remaining exit work is the ten-turn
hands-on playback, navigation, selection, and copy test.

### Slice B — Master charter and execution records

Create the versioned master VoiceOS charter, Rust goal/task/job/skill/automation schemas, migrations, and contract tests. Add IDs for every execution and artifact.

Implementation status (August 2, 2026): the shared charter is loaded by the
Python Ollama adapter, Rust gateway, and Codex bridge. Canonical owner-scoped
goal, project, task, job, skill, automation, artifact, provider-run, and
append-only execution-event records are implemented with contract tests. Token,
latency, tokens-per-second, cost, status, and provider/model telemetry are now
recordable. Capability leases, cancellation, and rollback metadata remain the
next trusted-kernel work.

### Slice C — Skill proposal prototype

Replay successful audit histories, identify one repeated read-only workflow, generate a proposed skill with required capabilities, validate it, and display the proposal in the approval UI. Do not execute generated skill content in this slice.

Implementation status (August 2, 2026): the Rust prototype reads the legacy
audit database without modifying it, requires repeated successful evidence,
extracts only typed capabilities, generates an inert review-only `SKILL.md`,
and deduplicates proposals by evidence hash. Generated content cannot execute,
and automation proposals require an approved skill. Gateway endpoints,
phone/touchscreen evidence cards, Hermes async approvals, and the governed
Hermes skill quarantine/validation/approval/rollback path are deployed.

## Consolidated remaining roadmap

The server foundation through shared owner conversation, live recovery/SSE,
Hermes async approvals, reviewed skills, ontology, task records, and private
documents is complete. Remaining work is ordered by dependency:

1. **Pixel and kiosk live-sync cutover:** consume active-conversation snapshot,
   persist an event cursor, reconnect with `after`, and render remote turns
   silently. Complete the Pixel ten-turn audio/copy/navigation reliability gate.
2. **Trusted job execution:** implement bounded capability leases, cancellation,
   checkpoints, idempotent resume, rollback metadata, and live execution/provider
   telemetry cards.
3. **HP wall-terminal milestone:** inventory and bootstrap Ubuntu, cold-boot into
   Carbon Command, validate touch and full-duplex USB audio, then add device
   presence and playback transfer.
4. **Streaming voice latency:** keep the local model warm, stream recognition and
   response deltas, move audio to Opus/WebRTC, add VAD and barge-in, and introduce
   server-quality speech only after interruption is reliable.
5. **Knowledge and proactive execution:** add attributed structured memories,
   full-text/semantic/temporal/graph retrieval, contradiction handling, task
   breakdown, timers, quiet hours, and reviewed automations.
6. **Operational hardening and Rust parity:** device revocation/rotation,
   retention/export/delete, encrypted backup/restore drills, crash recovery, and
   remaining Python route parity before moving the database to the HP.
7. **Multi-agent collaboration:** deploy Buzz only when the Rust coordinator can
   schedule multiple bounded agents with budgets and stop conditions. Buzz must
   transport signed collaboration events, never own VoiceOS memory or policy.

## Measures of success

- response playback success rate and time to first audio;
- conversation-continuity recall accuracy;
- memory retrieval latency and citation accuracy;
- percentage of tasks reduced to a clear next twenty-minute action;
- skill proposal acceptance, rejection, correction, and rollback rates;
- automation completion and interruption rates;
- model latency, tokens per second, token totals, GPU memory, and cost;
- number of privileged operations attempted, approved, denied, and blocked;
- user correction rate and time required to recover from an incorrect interpretation.
