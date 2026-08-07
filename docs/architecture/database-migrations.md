# VoiceOS authoritative database migrations

The Rust control plane records every authoritative SQLite schema migration in
`schema_migrations`. Each migration has a monotonically increasing version, a stable name,
a checksum, and an application timestamp.

Migration 1 adopts the complete pre-ledger VoiceOS schema. This makes both fresh databases and
existing rig databases converge on the same baseline without deleting or rewriting runtime data.

## Rules

- Never edit an already deployed migration to introduce new schema behavior.
- Add a new numbered migration and a new checksum for every later table, index, trigger, or
  column change.
- Apply one migration inside one `BEGIN IMMEDIATE` transaction.
- Write the ledger entry in the same transaction as the schema change.
- Stop startup when a recorded name or checksum differs from the binary's migration manifest.
- Keep migrations forward-only. Operational rollback restores the database backup made before
  release installation; it does not attempt lossy down-migrations.
- Test fresh creation, upgrade from the preceding version, failure rollback, and database reopen
  for every new migration.

The release installer should continue taking and validating the protected database backup before
starting a binary containing new migrations.
