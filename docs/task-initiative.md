# VIC proactive task intake

Every task created through voice, Android, the web interface, or the wall terminal enters the same Rust-owned intake path.

1. Rust stores the task and immediately derives a bounded capability scope and concrete next actions.
2. Rust creates one idempotent initiative job and records `task.initiative.queued` evidence.
3. The Python compatibility gateway atomically claims that job and asks Hermes, acting as VIC, to perform useful safe work.
4. Typed tools and their existing policies remain authoritative. Research, analysis, drafting, organization, and inspection can proceed automatically. External communication, purchases, destructive changes, credentials, and administrative actions require explicit approval.
5. Results or approval requests are audited, persisted against the task, and published as `task.initiative.updated` events to every connected client.

Task text is treated as untrusted data when sent to Hermes. It cannot expand the job capability scope or override approval policy. Claiming is atomic, so retries and multiple connected devices cannot run the same initiative twice.
