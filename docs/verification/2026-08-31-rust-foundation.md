# Rust foundation verification

Captured: 2026-08-31
Scope: Phase 1 of the stabilization roadmap.

## Environment adjustment

The roadmap's initial `/tmp` target directory had a 15 GB tmpfs quota with only 4.9 GB free. Parallel Rust target directories exhausted that quota before a governance test and the first workspace attempt could complete. Those temporary targets were created by this verification and removed. The successful final verification used the isolated cache target below on `/home`, which had 869 GB free. This does not alter repository source or tracked configuration.

The shell PATH did not expose `cargo` during the cache-target command session, so the verified commands used the installed Cargo binary directly: `/home/vic/.cargo/bin/cargo`.

## Focused behavior evidence

All commands ran from the repository root using `CARGO_TARGET_DIR=/home/vic/.cache/voiceos-stabilization-target`, `CARGO_INCREMENTAL=0`, and `CARGO_PROFILE_DEV_DEBUG=0`.

- `cargo test -p voiceos-core --test conversation_areas`: 7 passed before the lint correction; the final workspace run contains 8 passing tests after the added idempotency-conflict regression.
- `cargo test -p voiceos-core --test execution_persistence`: 4 passed.
- `cargo test -p voiceos-core --lib governance::tests::`: 3 passed.

During linting, Clippy found `clippy::collapsible_if` in `services/voiceos-core/src/conversation_store.rs`. Before refactoring that existing rejection branch, a regression test was added in `services/voiceos-core/tests/conversation_areas.rs` proving a request ID already used for conversation creation cannot be reused for a move operation. The focused test passed both before and after the behavior-preserving refactor.

## Final commands and results

```text
CARGO_TARGET_DIR=/home/vic/.cache/voiceos-stabilization-target \
CARGO_INCREMENTAL=0 CARGO_PROFILE_DEV_DEBUG=0 CARGO_BUILD_JOBS=2 \
/home/vic/.cargo/bin/cargo test --workspace
exit status: 0

CARGO_TARGET_DIR=/home/vic/.cache/voiceos-stabilization-target \
CARGO_INCREMENTAL=0 CARGO_PROFILE_DEV_DEBUG=0 CARGO_BUILD_JOBS=2 \
/home/vic/.cargo/bin/cargo clippy --workspace --all-targets -- -D warnings
exit status: 0

/home/vic/.cargo/bin/cargo fmt --all -- --check
exit status: 0

git diff --check
exit status: 0
```

The final workspace run completed in 0.55 seconds from cache and included the Rust core, Rust gateway, ontology, desktop clients, integration tests, and doc tests. Notable current counts reported by the command: 10 core unit tests, 8 conversation-area integration tests, 4 execution-persistence tests, 39 gateway tests, and 11 ontology tests, with all displayed suites passing.

## Outcome

Phase 1 is green on the current working tree: Rust formatting, all focused core behavior groups, the full workspace suite, strict Clippy, and whitespace validation all pass. The next required gate is migration safety against a representative prior-schema database; it has not been inferred from fresh-schema tests.
