# Phase 3: Workflow improvements

## Outcome

Make the proven import/review/execute loop faster without weakening review or
payment safety.

## Candidate improvements, in priority order

1. Drag-and-drop CSV upload and import summary in the local interface.
2. Optional watched inbox: wait for complete writes, hash inputs, archive originals,
   and deduplicate repeat events. Watching/importing never approves deposits.
3. Better rule suggestions and explicit merchant aliases, with preview, provenance,
   conflict reporting, and reversible changes to unsubmitted decisions.
4. Keyboard shortcuts, saved filters, and improved bulk-review ergonomics.
5. Optional read-only Monzo matching to suggest manually handled purchases. Equal
   amounts/dates are evidence, not proof; ambiguous matches require confirmation.
   Respect pagination and the documented history-access limits.
6. Optional CSV/spreadsheet audit reports and backup/restore UI.
7. Explore Proton Drive acquisition only if a supported, secure integration is
   available and worth the complexity. Virgin-app export stays manual unless a
   suitable supported route is established.

## Constraints

- Never auto-execute because a file appeared in an inbox.
- Do not add hosted services, AI-based classification, a Node build pipeline, or
  OAuth token persistence without a separate decision.
- Avoid syncing an open SQLite database through Proton Drive. Back up consistent
  snapshots and source archives instead.
- Keep all-transactions inspection and manual overrides available.

## Acceptance criteria

- Repeated uploads/watch events produce no duplicate purchases or operations.
- Interrupted file copies cannot create a misleading successful import.
- Suggestions never silently override explicit user decisions.
- Monzo matching never silently labels an ambiguous purchase paid.
- A normal recon requires only source acquisition, import, exception review,
  approval, and execution—not copying or exporting spreadsheet sheets.
