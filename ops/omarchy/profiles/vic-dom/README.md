# VIC-DOM Omarchy profile

This profile prepares a separate VoiceOS deployment for the Brick and Copper
restaurant computer.

- **DOM** remains the restaurant's Digital Operations Manager and the authority
  for existing restaurant state and workflows.
- **VIC** is the Voice Interface Controller through which people speak to DOM.
- **Hermes** is the reasoning runtime behind VIC.
- **VoiceOS** supplies local identity, memory, tasks, permissions, and audit.
- **Touch** is the local touchscreen interface.

The restaurant build accepts voice and direct Touch controls only. Typed entry
and in-kiosk connection editing are removed from this profile. A physical
keyboard remains available outside the kiosk for maintenance and recovery.

The profile does not contain credentials, restaurant records, personal VIC
memory, or data from another computer. It does not replace an existing Hermes
identity file. When an existing file differs, the installer leaves a proposed
file beside it for review and refuses to enable services. Merge the proposed
VIC-DOM section into the existing Hermes file while retaining its heading; the
installer recognizes that heading on the next run.

## Before transfer

1. Finish installing and configuring Hermes on the destination Omarchy computer.
2. Keep the existing DOM application and its data in place.
3. Copy the VoiceOS repository without any `.env`, key, database, model, build,
   or personal Hermes-state files.
4. Run the read-only check:

   ```bash
   ops/omarchy/profiles/vic-dom/preflight.sh
   ```

## Installation command

After the preflight passes and the existing DOM application has been backed up:

```bash
ops/omarchy/setup.sh --profile vic-dom --skip-tailscale
```

Private phone access stays out of the first installation. Configure device
authentication and Tailscale only after local Touch, microphone, speaker, DOM
integration, and approval checks pass.

## Files installed by the profile

- `~/.config/voiceos/gateway.env`
- `~/.config/voiceos/core.env`
- `~/.config/voiceos/deployment-context.md`
- `~/.hermes/SOUL.md` when no existing file is present
- `~/.hermes/workspace/AGENTS.md` when no existing file is present

The Touch build also consumes `ui-build.env`, which selects the `vic-dom`
identity and the `voice-touch` input mode at compile time.

VoiceOS state is created fresh on the destination machine under
`~/.local/state/voiceos`. Existing DOM data is not copied into that directory.

## Deliberate exclusions

Never include these in a transfer bundle:

- `~/.local/state/voiceos` or repository `work/` databases
- `~/.config/voiceos/*.key`, `*.env`, or device enrollment tokens
- `.hermes/`, `~/.hermes/`, Codex credentials, or provider credentials
- `.git/`, `target/`, `node_modules/`, `.next/`, `dist/`, or Python caches
- exported employee, payroll, payment, customer, or vendor data

Restaurant integrations are connected only after their location, ownership,
backup, and permission boundaries have been reviewed on the destination.
Use `restaurant-data-sources.yaml.example` for that inventory and
`voice-command-learning.md` for the reviewed voice-learning lifecycle.
