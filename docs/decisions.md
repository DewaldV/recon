# Decision log

Recorded 2026-11-01. **Agreed** entries capture user-approved direction.
**Preparation default** entries are concrete technical defaults selected to make
Increment 1 implementable; they can be revised with an explicit follow-up entry.
Future changes should supersede entries rather than erase their rationale.

| ID | Status | Decision and rationale |
| --- | --- | --- |
| D001 | Agreed | Build a new standalone `recon` project under `~/Projects/dewaldv`; leave `monzo-cli` and the existing workflow intact. Port useful code selectively rather than transform the CLI or extract a shared library now. |
| D002 | Agreed | Use Rust, a single local application, and SQLite as the persistent ledger. No hosted service or separate frontend service. |
| D003 | Agreed | Use devenv + direnv for tools. No Node/npm frontend tooling. |
| D004 | Agreed | Use Axum + Askama server-rendered HTML + HTMX. Serve a pinned HTMX asset locally, retaining its license; use ordinary CSS and minimal vanilla JavaScript only where necessary. No CDN or bundler. |
| D005 | Agreed | Optimize for exception review, persistent decisions, bulk operations, and explicit approval—not a spreadsheet clone. All transactions remain inspectable. |
| D006 | Agreed | Keep import/review offline and credential-free. Phase 1 cannot move money. Monzo execution is a separately validated Phase 2 capability. |
| D007 | Agreed | Keep categories locally; use descriptions as Monzo notes. Preserve one deposit per purchase. The documented API does not offer category updates. |
| D008 | Agreed | Retain manual Virgin export/Proton transfer initially; automate downstream work first. Inbox watching is optional later and must never trigger payment approval. |
| D009 | Agreed | Keep source data, databases, backups, and secrets outside the source repository. Read legacy files without modifying them; commit synthetic fixtures only. |
| D010 | Agreed | Preserve merchant/account decisions with provenance. Merchant category/description rules and Personal/Joint rules are separate. Activate historical suggestions only after approval; explicit overrides win. |
| D011 | Agreed | Use integer pennies and strict parsing. Use source identity and persistent handling state rather than `lastpostingdate.txt` as duplicate/payment authority. Surface refunds/fees/ambiguous rows rather than silently discarding them. |
| D012 | Agreed | Phase 2 uses persisted stable Monzo deduplication keys, immutable approved instructions, explicit confirmation, and separate deposit/note progress. Never repeat a confirmed deposit just to repair a note. |
| D013 | Agreed | Keep credentials backend-only, injected through `pass-cli run` for online work. Never persist tokens or expose them to browser storage or logs. |
| D014 | Agreed | First increment is CSV import, transaction table, individual Personal/Joint edits, persistence/restart, and safe reimport. Defer merchant rules, historical decisions migration, bulk editing, exports, and all Monzo access. |
| D015 | Preparation default | Scope a nonempty Virgin reference to a configured source-account identity. Preserve references as text, including leading zeros. Reuse identity only after checking canonical financial fields; quarantine missing references and conflicting observations. Never use a merchant/date/amount fingerprint as automatic identity. See the investigation. |
| D016 | Preparation default (implemented) | Version SQL migrations; use SQLite STRICT tables, foreign keys enabled on every connection, and transactional migrations. Use `rusqlite` with bundled SQLite and the online backup API. |
| D017 | Preparation default | Default to `$XDG_DATA_HOME/recon`, falling back to `$HOME/.local/share/recon`; require absolute non-repository, non-cloud-synced paths for overrides. Create private data directories/files. Separate development databases from real data. |
| D018 | Preparation default | Bind IPv4 loopback `127.0.0.1:7310` only. Use a per-start random access token exchanged for an HttpOnly, SameSite=Strict session cookie; CSRF tokens plus exact Host/Origin validation protect mutations. No sensitive HTTP route is available without a session. See the security contract. |
| D019 | Preparation default | Import full source exports without a posting-date cutoff, but label execution history unknown. Account assignment alone is not readiness to pay; old purchases remain ineligible for execution until explicit later disposition. |
| D020 | Preparation default | Source observations and review decisions are append-only. A repeated identical file is a no-op per source account; overlapping files add provenance without overwriting choices. Keep raw file bytes in SQLite initially for transactional archival and consistent backups. |

## Implementation refinements

### D021 — Browser-compatible referrer policy (implementation default, 2026-11-01)

Use `Referrer-Policy: same-origin`, superseding the prepared `no-referrer` default.
The Firefox smoke test showed that `no-referrer` makes the Origin of ordinary
multipart form navigation null. Keep exact Origin validation rather than accepting
null Origins; `same-origin` still prevents referrer disclosure to other origins.

## Additional agreed decisions

### D022 — One review workspace (agreed)

Consolidate individual account allocation and merchant/bulk review into `/review`.
Keep `/purchases` as an authenticated bookmark redirect, not a second screen.
Inline edits retain version/conflict checks and refresh counts, filter membership,
and bulk versions together. This adds no handling state or financial execution.

### D023 — Meaning of legacy Paid (user clarified)

`Paid = Yes` means the purchase was handled outside this recon process. For
example, the user's partner purchased on the credit card and independently moved
the corresponding money into the Monzo credit-card pot. Recon must not deposit
for that purchase again. It does **not** mean the credit-card bill itself was repaid.

Increment 3 should map a confirmed matching legacy Yes to **handled externally**,
not generic exclusion: preserve the purchase and its audit history, but exclude it
from outstanding totals, executable exports, and deposits across all recon workspaces.
A generic exclusion remains a separate state with its own reason. Confirm the
source-row match and provenance before migration; this meaning does not resolve
ambiguous matches or prove that `Paid = No` remains outstanding today. No existing
purchase handling state is changed by recording this clarification.

## Further implementation defaults

### D024 — Increment 3 delivery slices (implementation default)

Deliver the large increment in the documented slices, starting with handling,
explicit recon selection, and totals. Historical migration, source-exception
repair, account rules, and export/handoff remain separate later slices. Do not
label the whole increment complete or offer payment/export operations early.

### D025 — Handling and recon scope safeguards (implementation default)

Unknown is a virtual initial handling state, not a fabricated historical decision.
Manual state changes need evidence notes and preview/confirmation. Reopening any
purchase with prior external/excluded history requires explicit confirmation,
even after an intermediate unknown state. Global handling survives imports and
workspace changes. One purchase may belong to only one active recon; archive and
membership events never settle it. Archived selections retain history but are
never ready. SQL constraints and audit triggers back these domain rules.

### D026 — Single queue and automatically recorded runs (agreed)

Supersedes the named-workspace scope in D024/D025. All outstanding purchases belong
to the next recon. Personal and Joint are grouped within one run; filtering and
pagination never determine execution scope. Only one run can be active. Existing
workspace records remain read-only history and do not confer staging approval.

### D027 — Versioned staging and exact rules (agreed)

Staging explicitly confirms outstanding funding and the effective account,
category and note. Relevant source/decision changes invalidate staging. Defaults
never stage automatically. Account precedence is explicit purchase choice,
merchant+card rule, merchant rule, then missing. Clearing a purchase choice returns
to rule fallback. All outstanding blockers and source exceptions must be resolved
before starting. Save and stage is one atomic transaction.

### D028 — Temporary-token execution and manual note matching (agreed/default)

Supersedes `pass-cli` injection in D013. The user supplies a temporary token through
the local form for each execution/resume/note attempt. The backend holds it only
in memory; never persist or return it. Immutable instructions and keys precede
remote requests, confirmed deposits are skipped on resume, and uncertain retries
retain their original identity. Deposit confirmation removes a purchase from the
outstanding queue, but a run completes only when its notes also complete.

Monzo deposits return pot state without a transaction ID. The current implementation
requires an explicit user-confirmed transaction match and validates account scope,
currency and amount. It never chooses a transaction based on equal amount/time.
Reserve the mapping before annotation and reuse it after failure. Automatic
correlation and live API verification remain unresolved.

### D029 — Migration, source dispositions and restore guard (implementation default)

Historical CSV rows must match verified Virgin references. Preserve raw bytes,
proposals and prior snapshots; never overwrite existing explicit decisions or run
instructions. Paid Yes can establish external handling when no handling decision
exists; Paid No cannot establish outstanding status. Neither stages purchases.
Source dispositions retain canonical purchases/reject conflicting observations or
explicitly exclude non-purchase observations; they do not manufacture repaired rows.

Persist an independent execution watermark to detect stale database restores.
Missing/stale recovery evidence blocks execution; restoration of both old database
and old watermark still requires external reconciliation. See the restore guide.

## Remaining open decisions

- Merchant pattern/rule language and historical suggestion thresholds.
- Automatic Monzo transaction correlation for notes and live validation of explicit matching.
- Refund/fee financial treatment and any future withdrawal support.
- Any Proton acquisition integration or persistent OAuth workflow.
