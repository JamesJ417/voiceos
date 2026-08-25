# DOM voice-command learning

DOM learns reviewed vocabulary and proven workflows; it does not silently turn
an unfamiliar sentence into a new permission.

## Learning loop

1. VIC records the local speech transcript and asks the VoiceOS ontology for a
   canonical interpretation.
2. A high-confidence known command may use its existing typed capability.
3. A low-confidence or unrecognized phrase is kept as an audited interpretation,
   not executed as a guessed command.
4. Touch presents the heard phrase, proposed meaning, target, and consequence.
5. An authorized manager approves or corrects the meaning by touch.
6. VoiceOS stores the correction or approved alias only inside the VIC-DOM owner
   scope. It never becomes a global capability.
7. The phrase is replay-tested against the typed capability before it is treated
   as learned. The mapping can be disabled or replaced without deleting history.

Examples of vocabulary DOM may learn after review include local names for a
station, shift, checklist, vendor, menu period, storage area, or report. A new
action—such as placing an order or publishing a schedule—still requires a
reviewed adapter, explicit capability, and the applicable touch approval.

## Touch-only confirmation

Voice can request work and explain a correction, but voice cannot authorize a
pending consequential action. Approval and denial are explicit Touch controls
bound to the authenticated device and audit record.

## Current platform foundation

VoiceOS already records interpretations, supports corrections, and stores
owner-approved entity aliases. The next destination-side step is to expose the
restaurant-specific catalog and correction queue in Touch after DOM's actual
data entities and APIs are inventoried.
