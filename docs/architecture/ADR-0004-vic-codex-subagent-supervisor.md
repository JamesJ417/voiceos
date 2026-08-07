# ADR-0004: VIC-governed Codex subagent supervisor

- Status: Accepted for incremental implementation
- Date: 2026-08-06
- Scope: Coding work delegated by VIC to Codex coordinator and subagent threads

## Decision

VIC remains the user-facing planner, task owner, memory authority, and approval authority. A
separate local supervisor runs Codex for coding work. Codex may create its own bounded subagents;
local models and Hermes may propose that coding work is useful, but they cannot execute it or
select broader permissions.

The supervisor will use `codex app-server` over local `stdio`. App-server is intended for deep
product integrations requiring authentication, history, approvals, and streamed agent events.
Its local standard-input/output transport avoids exposing the experimental WebSocket listener.
The supervisor must generate or validate against the protocol schema shipped by the installed
Codex version.

References:

- [Codex App Server](https://learn.chatgpt.com/docs/app-server)
- [Codex subagents](https://learn.chatgpt.com/docs/agent-configuration/subagents)
- [Codex authentication](https://learn.chatgpt.com/docs/auth)

## Execution model

1. VIC creates a top-level `agent_run` linked to a VoiceOS task and immutable capability scope.
2. The Rust control plane persists it as `queued` and emits `agent.run.queued`.
3. The local supervisor atomically claims the run and starts a Codex coordinator thread.
4. The coordinator receives the objective as untrusted task data plus fixed VoiceOS developer
   instructions, repository path, sandbox, model, and reasoning effort.
5. Codex may delegate independent work to subagents. Each observed `collabToolCall` becomes a
   child `agent_run` with the same task, no broader sandbox, and a subset of the parent's
   capabilities.
6. Plan, command, file-change, approval, subagent, and terminal events are normalized into the
   VoiceOS execution timeline and streamed through the existing SSE connection.
7. The parent run cannot complete until its required child runs are terminal and verification
   evidence is recorded.

## User experience

Phone and kiosk clients show an Agent Activity projection:

- top-level objective and linked task
- coordinator and child-agent tree
- queued, running, waiting, completed, failed, or cancelled status
- current activity and latest plan step
- commands, file changes, test evidence, and final summaries
- approvals requiring the user
- stop, inspect, continue, and review-result actions

The UI consumes Rust records, not raw Codex process output. Raw output remains bounded diagnostic
evidence and is never treated as authoritative VoiceOS memory.

## Permission policy

- Initial runs allow only `read-only` and `workspace-write` sandboxes.
- `danger-full-access` is rejected by the Rust model and database constraint.
- Subagents inherit the parent's task, working tree, approval policy, and maximum permissions.
- Local models and stored task text cannot modify model, reasoning, sandbox, capability scope,
  approval policy, or repository root.
- Shell and file approvals become existing VoiceOS approval cards before execution.
- Privileged operating-system actions remain behind the separate root-action broker.
- Cancellation is durable in Rust and the supervisor must interrupt the matching Codex turn.

## Model policy

The initial coordinator and workers use `gpt-5.6-sol` with high reasoning. Later, explicitly
read-only discovery workers may use a faster Codex model, but implementation, integration,
security review, and final verification remain on the highest-quality configured Codex tier.

## Failure and recovery

- Creation is idempotent by owner and idempotency key.
- State transitions use compare-and-swap semantics.
- Terminal runs reject late progress events.
- On supervisor restart, `starting` and `running` records are reconciled against persisted Codex
  thread IDs before retrying.
- A retry resumes the existing Codex thread when possible; it never creates a second run with the
  same idempotency key.
- Child runs cannot point at a different task than their parent.
- Completion, failure, and cancellation remain visible even if a phone or kiosk disconnects.

## Rollout

1. Durable run records, parent-child invariants, REST contract, and timeline events.
2. Local app-server supervisor with schema-pinned event normalization.
3. Approval mapping and interruption/cancellation.
4. Phone and kiosk Agent Activity views.
5. Read-only trial, workspace-write trial, recovery test, then guarded production enablement.
