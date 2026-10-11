# Accounts, revisions, approved data, and dataset versions

The application follows one pipeline:

```text
User → Project → Working copy → Submitted revision → Review → Approval
                                                             ↓
                  PyTorch / training ← Dataset version ← Approved data
```

## Accounts and project access

First launch creates an administrator through the sign-in screen. There is no default application password. The compact sidebar user section shows the signed-in name and account groups. Its menu contains Account settings, Role and permissions, Admin settings when permitted, appearance controls, and Sign out.

Account settings manages the user's display name and password. Role and permissions shows account grants and effective permissions for the open project. Admin settings manages users, groups, project assignments, and review policy. Groups are editable permission sets, rather than special hard-coded role names. Users inherit account groups; project membership adds local groups/grants and can restrict inherited permissions. Ordinary users need explicit membership. `projects.admin` permits access across projects without membership; individual operation permissions still apply.

Administrators can create/edit users, set groups/direct permissions, assign projects, reset passwords, disable accounts, and delete accounts. Deletion requires a confirmation in the UI. Deleted accounts become minimal identity tombstones: credentials and grants are removed, sessions are revoked, and the user disappears from assignment choices. Historical annotation, review, comment, and dataset actor names remain recorded. The backend prevents disabling or deleting the last enabled administrator, including through permission/group changes.

A project manager can manage memberships from Members. The user administration form's Project access pane changes only the selected user's assignments and preserves other members' grants.

| Permission | Operations |
| --- | --- |
| `projects.admin` | Create projects, browse/open filesystem paths, access projects without membership |
| `project.view`, `project.manage` | Read project metadata; manage members, recordings, storage upgrades, and processing |
| `mcap.view`, `annotation.view` | Open recording data; inspect annotations and revision history |
| `annotation.create`, `annotation.edit`, `annotation.delete` | Create, modify, or delete working annotation rows |
| `annotation.submit` | Submit working-copy revisions |
| `review.view`, `review.assign` | Review queue, claim reviews, assign reviewers/working annotators |
| `review.approve`, `review.reject`, `review.comment` | Approve a whole revision, request another revision, write feedback |
| `labels.manage` | Label definitions, class identities, label lists |
| `datasets.generate`, `datasets.export` | Create/build versions and configure CSV label tables; download/export training data |
| `users.manage`, `groups.manage`, `settings.manage` | Account, group, and policy administration |

The CSV annotation editor requires `datasets.generate`; grant this to annotators who label structured messages. The backend enforces permissions independently of visible UI controls. Application permissions do not replace operating-system permissions for the normal project folders.

## Working copies and immutable submissions

Annotation tools always edit a working copy. Saving a box, class, or CSV value does not change any submitted snapshot or approval. **Save and submit revision** publishes a new revision every time, including when the content is unchanged. Annotation tasks can cover complete topics or frame ranges; revision references include the task identity and coverage, so separate tasks from one MCAP remain unambiguous.

```text
Not started → Annotating → Submit revision → Ready for review
                                                 ↓
                                          Reviewer starts
                                                 ↓
                                           Under review
                                           ↙          ↘
                              Changes requested      Approved
                                      ↓
                       Edit working copy → Submit new revision
                                                ↓
                                         Ready for review
```

There is no Updating review state. A new submission supersedes an unfinished older review, without moving its comments or replacing its content. An approved older revision remains approved when a new working copy or submission appears. Review policy defaults to a reviewer other than the actual revision submitter. Reassigning working-copy ownership does not rewrite earlier submitter identities.

Each revision records submission time/person, source recording SHA-256, covered ranges, label definitions, annotation pages, CSV column/value pages, topic index, and processing configuration. Existing immutable JSON pages are pinned into review storage; raw MCAP files are never copied or rewritten for a submission. Equal pages/snapshots are reused. Working UI positions and playback step preferences are excluded from review content.

## Review interface

Project MCAPs focuses on file browsing, import, and removal. Each row shows one compact status link for its most recent submitted revision, or Not submitted when it has none. Clicking the status opens the corresponding review. Assignment controls, review feedback, and the workflow viewer live in Reviews.

Reviews lists all open submissions, including work assigned to other reviewers and revisions waiting on requested changes. It displays MCAP name, revision, status, annotator, reviewer, comment count, and submission date. Status/search filters and Assigned to me narrow the queue; Approved and All submissions include closed decisions. Separate topic/range tasks from one MCAP remain separate queue entries, so no open review is hidden by a newer submission in another task.

Reviewer assignment uses a searchable dialog with a bounded, internally scrolling list. It includes enabled users with effective `review.view` permission for the project. Type a display name or username; matching ignores case, accents, punctuation, and spacing. Your own account is marked You and can be selected for your own submission. Users with `review.view` can assign an available revision to themselves; assigning other users, clearing assignments, or taking a revision assigned to someone else requires `review.assign`. Reassignment returns an open revision to Ready for review and preserves its snapshot and comments. Closed revisions cannot be reassigned.

Explicitly selecting yourself for your own submission records `self_review: true` in that revision's review metadata and audit history. This permits self-review even with the default independent-review policy enabled. The policy remains in place for automatic claims and assignments by other people; no workspace setting is changed. Reassignment clears this authorization, and a different submitter does not inherit it. Project review and decision permissions still apply: annotation permissions alone cannot review or approve a revision.

The review page keeps a compact title, reviewer selector, and History button above the video and decision panel. History contains past revisions, comparisons, and working-copy assignment. Its user picker is filtered to enabled project members with `annotation.submit`; only `review.assign` holders can change this owner. Ownership changes preserve submitted annotations, comments, review decisions, and dataset references.

The API stores Ready for review as `submitted` and Under review as `under_review`. Older unstarted `under_review` records are interpreted as waiting submissions when read, without rewriting their files. The status overview reads review metadata; it does not scan recordings or load annotation snapshots.

Opening a submission navigates to a dedicated review page with its immutable annotation viewer, previous feedback, and current feedback. The queue remains a separate page. Annotators can read submissions and feedback without gaining reviewer permissions. The reviewer can:

- **Approve Revision** immediately, without playing the video or visiting every frame.
- Add multiple frame-range or general comments, then **Request Changes** for the complete revision.

Playback starts the assigned or available review automatically when the viewer has review permission. The decision buttons also record the claim/start transition when needed, so approval requires no playback. The backend continues enforcing assignment, permissions, and revision concurrency. There are no frame-level approval controls or playback-completion requirements.

Playback and scrubbing cover the full source topic, including context outside a submitted segment. Annotations and CSV values come from the selected revision; frames outside submitted ranges are marked as recording context. Frame references identify feedback locations and do not make per-frame approval decisions. The original MCAP remains unchanged.

Single-frame feedback uses the same first and last frame. Clicking a feedback range seeks the timeline immediately. Draft comments are submitted together; stored comments contain the actor, timestamp, and covered range. Feedback permanently belongs to its revision. Previous revisions and closed decisions are read-only.

The separate revision history lists earlier snapshots and their submission/review timestamps, people, decisions, and comments. Opening a revision shows its read-only annotations. Previous feedback appears beside a newer revision as context and remains labelled with its original revision. Comparison shows added/deleted/modified frames, old/new annotation values, and changes to label definitions, ranges, or CSV content. Concurrent decisions use revision-specific optimistic versions, independently of working-copy save events.

Legacy task histories are migrated without inventing annotation data. A latest old snapshot is recovered only if its original digest proves that the current working copy matches. Older events/comments remain visible as history; an unavailable old snapshot is marked explicitly. All new submissions retain complete immutable content.

## Approved data browser

A whole approved revision becomes available immediately in Approved data. Pending revisions and working edits are excluded. The browser supports reciprocal label/modality filters, MCAP search, project, annotator, reviewer, annotation type, approved revision, and sorting by label/modality/source/sample count/date/revision. The default shows the latest approved revision of each task; earlier approvals remain selectable. Tables are paginated.

Labels report directly annotated sample counts. Modalities include direct annotation streams and associated inputs from the same source MCAP within the approved time coverage. Sensor rows marked synchronized input represent raw input records, rather than additional labelled targets. RGB/depth/IMU/joint/pose/force/point-cloud categories are inferred from topic/schema/encoding; otherwise data is shown as generic sensor messages. Actual source topic names are retained.

Creating a dataset selects approved revisions, target labels, input modalities, target stream, negatives, and synchronization. The default uses the reviewed configuration, with nearest matching and a 100 ms tolerance when no tolerance was configured. Explicit nearest/linear/rate rules are recorded in the version recipe. Sensor windows are optional and capped at ten seconds/4096 records per window. Incompatible or out-of-tolerance sources are excluded and reported. Counts before creation are estimates; published membership records the exact count.

Cross-project discovery exposes only projects accessible to the signed-in user. Including an external project's sources requires its dataset export permission. Registered source projects must remain available to rebuild missing derived artifacts. A built version loads from its own project-local artifacts without a running server.

## Dataset versions and reproducibility

Every creation or explicit **Create new dataset version** publishes a separate immutable definition and bounded sample pages. It records exact approved revision IDs/numbers/hashes, raw source identity, label definitions, frame membership, modalities, translations/synchronization, and creator/approval history. A later approval becomes additional available data; it never changes a previous dataset version. The UI indicates newer approved revisions when relevant.

CSV and YOLO builds read fixed version membership and frozen targets, never the working copy. Repeated builds of an existing version reuse its outputs; explicit repair rebuilds missing outputs. CSV is streamed to a temporary file. Images and standardized sensor JSONL are version-local artifacts. High-precision depth is preserved separately as standard NumPy tensors, including original units, stride, and endianness; display previews remain RGB8. Old caches recover native depth payloads once when a build needs them.

YOLO supports bounding-box targets with a selected primary image. Classification and structured targets use CSV/PyTorch. Timestamp synchronization associates inputs; it does not perform cross-camera spatial reprojection. Target coordinates retain their annotated camera's coordinate system.

Working CSV tables and unversioned legacy datasets remain separate from production versions. Under the default approval policy, legacy training builds ask for a pinned version from Approved data. Annotation exports and CSV label-table downloads use approved snapshots. Exploratory legacy policy compatibility remains available to trusted local callers; it does not make unapproved data eligible for production version creation.

```text
project/
  project.json
  access.json
  mcaps / labels / annotations / translations / synchronization
  reviews/<recording>/
    <task>.json                    working-copy coordination
    <task>/revisions/<number>.json revision-specific review metadata
    <task>/snapshots/<hash>.json   immutable snapshot references
    _pages/                       pinned annotation/CSV JSON pages
    _source/                      shared immutable source index
  datasets/<dataset>/
    dataset.json                  version summaries and latest pointer
    versions/<number>/
      version.json                immutable recipe and provenance
      samples/                    bounded fixed membership pages
      generated/csv or yolo/      rebuildable training artifacts
  audit/                          account access, working saves, reviews, datasets
  indexes/approved/               rebuildable discovery summaries
```

Accounts/groups/Argon2 hashes and expiring hashed session files belong to `workspace/data/accounts/`. Source recordings, annotation pages, review snapshots, and version definitions should be backed up. Caches, summaries, and generated artifacts can be recreated when the referenced source content remains available.

## PyTorch

Install `python/` with its training extras and build the desired version/format first:

```python
from mcap_project import Project
from torch.utils.data import DataLoader

project = Project.open("./my_project")
dataset = project.dataset("gripper_detection", version=1)
torch_dataset = dataset.to_pytorch()
loader = DataLoader(torch_dataset, batch_size=32, shuffle=True,
                    collate_fn=torch_dataset.collate_fn)
```

Omitting `version` selects the latest version when opening the dataset. An already opened dataset keeps its selected version. `dataset.provenance` exposes the approved source references. Versioned samples return a modality dictionary and targets, including the revision and exact timestamp. RGB loads as CHW float images; depth retains recorded values; sensor inputs use stable numeric feature schemas and optional windows. Variable target/window shapes are handled by the supplied collator. Explicit `feature_columns`/`target_column` select the tabular interface when a version contains CSV values.

Sample pages, images, tensors, and sensor records load lazily. Page/record/file-handle caches are bounded and reset for worker serialization. The Python reader requires no server for loading built versions.

## HTTP and verification

Sign-in/setup use HttpOnly, SameSite=Strict cookies. All modifying requests require `X-Requested-With: mcap-workbench`. Origins are checked; HTTPS deployments use `MCAP_SECURE_COOKIES=1`. New sessions use the configured lifetime; disabling/deleting a user or resetting credentials revokes access. Permission denials return 403 and authentication failures 401.

Revision APIs are under `/projects/{project}/tasks/{task}/revisions`; actions accept the revision number and expected revision version. Approved discovery is `/approved-data` or `/projects/{project}/approved-data`. Production creation is `POST /projects/{project}/dataset-versions`; subsequent versions use `POST /datasets/{dataset}/versions`. Version detail, samples, builds, input preview, and CSV downloads have version-specific routes. API frame indices start at zero; the UI starts at one.

Rust and Python tests cover access boundaries, permanent history, pinned data, depth precision, lazy workers, and no-op builds. Browser/API smoke checks use fresh temporary workspaces and independent accounts. `scripts/review-smoke.mjs` exercises account administration, review/resubmission/history/diff, approved data, version creation, and confirmed deletion. `scripts/playback-smoke.mjs` preserves the playback/import/save regression checks. `scripts/project-storage-smoke.py` tests real generated versions and PyTorch workers.
