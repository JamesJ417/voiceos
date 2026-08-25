# VIC-DOM permission plan

This is the review plan for future DOM adapters. Prompt text does not grant any
of these permissions; each capability must be implemented as a typed VoiceOS
tool with authentication, validation, audit, and approval enforcement.

The restaurant kiosk accepts voice requests and direct Touch controls only.
Voice can start a request, but consequential execution approval is an explicit
Touch decision. Administrator keyboard access remains outside the kiosk.

| Area | Read or prepare | Approval required before execution |
| --- | --- | --- |
| Daily operations | Status, checklists, draft handoff | Publish or close a shift record |
| Scheduling | Read availability, identify conflicts, draft schedule | Publish or change assigned shifts |
| Inventory | Read counts, flag variance, prepare order | Place, change, or cancel an order |
| Vendors | Read approved records, draft communication | Send messages or change vendor records |
| Money | Summaries and variance review | Purchase, payment, refund, payout, or account change |
| Employees | Assigned operational information | Discipline, hiring, termination, pay, or private-record changes |
| Customers | Approved service context | Outreach, refund, private-data export, or record deletion |
| System administration | Health and read-only diagnostics | Credentials, permissions, software, configuration, or destructive action |

Owner, manager, and staff roles must receive separate device identities. An
approval is bound to the authenticated person, exact action, exact arguments,
expiration time, and resulting audit evidence.

Every restaurant source begins read-only, identifies its dataset and freshness,
and returns provenance with each answer. Direct database writes and unrestricted
model-generated queries remain disabled.
