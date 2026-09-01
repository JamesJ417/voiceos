# VoiceOS stabilization baseline

Captured: 2026-08-31
Purpose: Preserve the exact starting point before stabilization edits. This record is evidence only.

## Repository identity

- Branch: `feature/vic-sleep-cycle`
- HEAD: `861a050 chore: checkpoint remaining sleep-cycle changes`
- Repository action taken: read-only inspection; no reset, clean, checkout, rebase, stash, staging, commit, or source modification.

## Working-tree status

Modified tracked files (18):

```text
apps/android/app/src/main/java/dev/voiceos/client/GatewayClient.kt
apps/android/app/src/main/java/dev/voiceos/client/MainActivity.kt
apps/android/app/src/main/java/dev/voiceos/client/ResumableResponse.kt
apps/android/app/src/main/java/dev/voiceos/client/VICConversationService.kt
apps/android/app/src/test/java/dev/voiceos/client/ResumableResponseTest.kt
contracts/openapi.yaml
contracts/route-ownership.json
services/gateway/server.py
services/voiceos-core/src/engine.rs
services/voiceos-core/src/lib.rs
services/voiceos-core/src/model.rs
services/voiceos-core/src/schema.rs
services/voiceos-core/src/store.rs
services/voiceos-gateway-rs/src/api/client.rs
services/voiceos-gateway-rs/src/api/conversations.rs
services/voiceos-gateway-rs/src/api/events.rs
services/voiceos-gateway-rs/src/api/mod.rs
services/voiceos-gateway-rs/src/api/turns.rs
```

Untracked files (12):

```text
apps/android/app/src/main/java/dev/voiceos/client/ConversationAreaModel.kt
apps/android/app/src/main/java/dev/voiceos/client/TtsResponseChunker.kt
apps/android/app/src/test/java/dev/voiceos/client/ConversationAreaModelTest.kt
apps/android/app/src/test/java/dev/voiceos/client/TtsResponseChunkerTest.kt
docs/client-distribution-tenant-security.md
services/voiceos-core/src/conversation_area.rs
services/voiceos-core/src/conversation_store.rs
services/voiceos-core/src/execution.rs
services/voiceos-core/src/governance.rs
services/voiceos-core/tests/conversation_areas.rs
services/voiceos-core/tests/execution_persistence.rs
services/voiceos-gateway-rs/src/api/executions.rs
```

Tracked diff statistic: 18 files changed, 2,583 insertions, 113 deletions.

## Integrity check

`git diff --check` exited successfully with no output (no whitespace errors reported).

## Toolchain evidence

```text
rustc 1.98.0 (88d9e12ae 2026-08-18)
cargo 1.98.0 (797e8a9bc 2026-08-05)
openjdk version "17.0.20.1" 2026-08-18
OpenJDK Runtime Environment (build 17.0.20.1+1)
OpenJDK 64-Bit Server VM (build 17.0.20.1+1, mixed mode, sharing)
```

Android build requirement: `apps/android/app/build.gradle.kts` sets both `sourceCompatibility` and `targetCompatibility` to `JavaVersion.VERSION_17`.

Android Gradle baseline without a session `JAVA_HOME` override:

```text
./gradlew --version
ERROR: JAVA_HOME is not set correctly and Java could not be found.
exit status: 1
```

Reproduced with the installed Java 17 configured only for the command session:

```text
JAVA_HOME=/home/vic/.local/share/mise/installs/java/17.0.2 \
PATH=/home/vic/.local/share/mise/installs/java/17.0.2/bin:$PATH \
./gradlew --version
Gradle 9.5.0
Launcher JVM: 17.0.2 (Oracle Corporation 17.0.2+8-86)
Daemon JVM: /home/vic/.local/share/mise/installs/java/17.0.2
exit status: 0
```

The earlier failure is an environment configuration issue, not a missing-JDK or source/test failure. Phase 4 must use this Java 17 command-session configuration for every Gradle verification command.

## Baseline conclusion

The candidate contains the recorded 30 affected files, remains uncommitted, and has no detected whitespace error. Android verification is not yet established because Gradle does not have a valid `JAVA_HOME` in the baseline session.
