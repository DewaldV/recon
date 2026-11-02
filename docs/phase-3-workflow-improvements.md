# Workflow improvements

Candidates for making import, review, and execution faster. Core gaps are listed
under [remaining work](single-queue-workflow.md#remaining-work).

## Candidates, in priority order

1. Drag-and-drop CSV upload.
2. Watched inbox: wait for complete writes, archive originals, and deduplicate
   repeat events. Importing must not approve deposits.
3. Historical rule suggestions with support counts, conflict previews, and approval.
   Exact aliases and manually approved account rules already exist.
4. Keyboard shortcuts, saved filters, and improved bulk-review ergonomics.
5. Read-only Monzo matching to suggest externally handled purchases. Respect
   pagination/history limits and require confirmation of ambiguous matches.
6. CSV/spreadsheet audit reports and backup/restore UI.
7. Proton Drive acquisition, if a supported integration justifies the complexity.

## Acceptance

- Repeated or interrupted inbox events cannot duplicate purchases or imply success.
- Suggestions preserve explicit decisions and require approval.
- Ambiguous bank matches cannot silently establish funding status.
- Manual inspection and overrides remain available.

Architecture and credential constraints are recorded in the [decision log](decisions.md).
