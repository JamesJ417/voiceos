# Client distribution and tenant security direction

The Rust core owns normalized tenant/user/device identity, authorization context, release metadata, and append-only audit event shapes. The gateway is the policy enforcement boundary: hosted deployments may resolve identity and grants centrally, while hybrid deployments may carry signed/attested context from a client and validate it at ingress.

This slice is deliberately descriptive scaffolding. Capability version `1` is the only currently recognized version; unknown versions fail closed. Release manifests carry version, key ID, and signature fields, but no cryptographic signing or verification service is implemented yet. Audit public projections omit sensitive detail by construction.

Deferred: key rotation and trust roots, signature verification, device attestation, grant persistence/revocation, replay/fencing, tamper-evident audit storage, update rollout policy, and gateway route integration. These should be added behind explicit contracts rather than inferred from these types.
