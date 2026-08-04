# Active conversation floor

VoiceOS has one server-owned active conversation per owner. Enrolled clients do
not create independent conversations. They compete only for a short-lived audio
floor that determines which device may capture microphone audio and play VIC's
reply.

## Lease behavior

- Touching **Talk** claims the floor and transfers it from any previous device.
- `VIC, continue here` is handled locally after the receiving device claims it.
- Listening, partial transcript, processing, and speaking updates renew a
  45-second lease through `POST /v1/conversations/active/floor`.
- A stale device receives HTTP 409 if it tries to update another device's lease.
- Stop, pause, cancellation, and completed kiosk playback release the floor.
- A crashed or disconnected holder is cleared by the Rust store after expiry.

Every successful change is audited in Rust as
`conversation.floor.changed`. During the Python-to-Rust migration, the Python
public gateway mirrors that change into the existing durable `/v1/events` SSE
stream. Android and Carbon Command consume the same event and immediately stop
local recognition and playback when another device becomes the holder.

Final user and VIC messages remain canonical conversation messages. The floor
only carries ephemeral presentation state and bounded partial text; it never
becomes a second conversation history.

## Handoff test

1. Enroll the Pixel and touch panel as separate devices.
2. Start Conversation Mode on the Pixel and verify the floor reports the Pixel.
3. Touch **Continue here** or **Talk** on the panel.
4. Verify the Pixel stops listening/speaking and the panel becomes the holder.
5. Speak on the panel and wait for VIC's answer.
6. Open History on both devices and confirm both turns appear under the same
   `conversation_id` from `GET /v1/conversations/active`.
7. Leave the active device disconnected for more than 45 seconds and confirm
   another client can claim the expired floor.
