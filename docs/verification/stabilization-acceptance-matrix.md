# VoiceOS stabilization acceptance matrix

Status meanings: Pass = evidence recorded from the current working tree. Fail = reproduced defect or environment blocker. Not run = no verification claim.

| Gate area | Procedure / evidence | Expected result | Owner | Status |
|---|---|---|---|---|
| Baseline preservation | `docs/verification/2026-08-31-stabilization-baseline.md`; `git status --short`, `git diff --stat`, `git diff --check`, branch and HEAD | Exact checkpoint and affected-file set recorded; clean diff check | VIC | Pass |
| Rust formatting | `cargo fmt --all -- --check` from repository root | Workspace formatter exits 0 | VIC | Pass — exited 0 after formatting the migration regression test on 2026-08-31 |
| Rust focused core tests | `TMPDIR=/var/tmp cargo test -p voiceos-core --test conversation_areas`; `TMPDIR=/var/tmp cargo test -p voiceos-core --test execution_persistence` | Required core behaviors pass | VIC | Pass — 9 conversation-area and 4 execution-persistence tests passed |
| Rust gateway HTTP tests | Full workspace includes `voiceos-gateway` HTTP and owner-isolation tests | Documented responses, state bounds, and ownership isolation pass | VIC | Pass — 39 `voiceos-gateway` tests passed in the full workspace run |
| Full Rust workspace suite | `TMPDIR=/var/tmp CARGO_TARGET_DIR=/var/tmp/voiceos-stabilization-target cargo test --workspace` | Actual workspace command exits 0 | VIC | Pass — exited 0 on 2026-08-31; all workspace unit, integration, and doc tests passed |
| Rust lint | `TMPDIR=/var/tmp CARGO_TARGET_DIR=/var/tmp/voiceos-stabilization-target cargo clippy --workspace --all-targets -- -D warnings` | Exits 0 | VIC | Pass — exited 0 |
| Migration upgrade tests | File-backed legacy-memory regression opens and migrates the database twice; verifies data preservation, `area_id`, and uniqueness | No loss or duplicate legacy memories; migration is idempotent | VIC | Pass for the legacy-memory regression — 1 focused test and the 9-test conversation-area suite passed. Exact-`861a050` fixture, legacy job preservation, and post-upgrade execution behavior remain gaps. |
| Database operations guidance | `docs/operations/database-upgrade-and-rollback.md` | Backup, compatibility checks, restore steps, and no automatic downgrade claim | VIC | Not run |
| OpenAPI validation | `python3 -m unittest contracts.tests.test_openapi_contract -v` | Route inventory, ownership, proxy coverage, schemas, and device security agree | VIC | Pass — 8 tests passed |
| Python gateway tests | Discover project runner, create approved isolated environment if required, run focused then available suite | Fieldy owner scope, errors, and audit behavior covered | VIC | Not run — the repository has the standard-library OpenAPI contract suite, but no separate Python gateway test runner was identified |
| Android Java environment | Java 17 configured for each Gradle command | Gradle detects Java 17 | VIC | Pass — Gradle ran with `/home/vic/.local/share/mise/installs/java/17.0.2` |
| Android unit tests | `JAVA_HOME=… ./gradlew test` | All changed model/client behaviors pass deterministically | VIC | Pass — Gradle test task exited 0 |
| Android debug build | `JAVA_HOME=… ./gradlew assembleDebug` | Debug build completes from verified source | VIC | Pass — assembleDebug exited 0 |
| Real-phone smoke flow | Approved test-data procedure in roadmap Phase 4.4, phone only | Conversation, area/thread, text/voice, interruption/resume, history/sync, safe execution path pass | James or authorized tester | Not run — needs phone-only verification |
| Security/design scope | Read-only review of governance types, gateway runtime, OpenAPI, and architecture/design documents | No claim that scaffolding is cryptographic/product security enforcement; documented contract/runtime enforcement must agree | VIC | Fail — device authentication defaults off and accepts caller-supplied identity; gateway-to-Rust proxy has no independently authenticated or transport-constrained boundary; enrollment request shapes are undocumented. This checkpoint is not a production tenant-security boundary. |
| Cross-stack contract smoke | Approved local test instances with authorized test data | Health, bootstrap, conversation, execution, and Fieldy responses match the contract | VIC | Not run |
| Final clean-diff review | `git diff --check`; inspect all candidate files against evidence | No credentials, generated artifacts, local data, unrelated edits, or undocumented limitations | VIC | Pass for whitespace and generated migration-fixture cleanup; full scope/security review remains pending |

## Environment note

SQLite file-backed tests fail intermittently when they use the default `/tmp` filesystem, which was 80% utilized and returned `disk I/O error` during the legacy-area test. The same test passes with `TMPDIR=/var/tmp`; all recorded Rust verification therefore uses that setting. The migration regression fixture now uses `std::env::temp_dir()` and removes its database, WAL, and shared-memory sidecars, so it does not write generated files into the source tree.

## Release decision rule

A stable internal-phone-test candidate requires every applicable row through the real-phone smoke flow to be Pass. A Not run row is never treated as Pass. Any explicit exclusion requires James’s approval and prevents a shipping claim for the excluded behavior.
