# Application architecture

MCAP Studio prepares MCAP sensor recordings for AI model training through processing, annotation, review, and versioned dataset generation. It is intended for teams using a shared host on a local network, with remote access through a VPN. React calls the Rust API over `/api`; the Rust backend owns file access, imports, processing, annotation persistence, dataset membership, and exports. No Python subprocess is used by the new app.

For team hosting, the Rust server serves the built frontend and API together, and project data stays in the host's workspace directory. VPN connectivity is provided by the team's network infrastructure. See the [README](../README.md#team-access-over-a-local-network-and-vpn) for setup and run instructions.

## Code organization

The Rust API and React application are organized by domain. The existing HTTP paths, on-disk project format, and public Rust `Store`, `Project`, `router`, and engine functions remain unchanged.

```text
backend/src/
  accounts/                  workspace users/groups, password hashing, sessions, project permissions
  api/                       router, middleware, errors, workers, resource handlers
  store/                     application operations and shared runtime caches
    auth.rs                  authentication, CSRF and route permissions
    accounts.rs              account and membership HTTP adapters
    reviews.rs               task and review HTTP adapters
    projects.rs              discovery, lifecycle, summaries, upgrades
    recordings.rs            imports, previews, workflows, pipeline publication
    annotations.rs           annotation mutations and geometry/class validation
    labels.rs                named entities, label definitions, label lists
    datasets.rs              membership, inspection, numeric analysis
    dataset_generation.rs    legacy CSV/YOLO materialization
    dataset_versions/        fixed production membership, synchronization, materialization
    structured.rs            message flattening, columns, CSV creation
    reviews/                 working copies, immutable revisions, feedback/diff, approved catalog
    exports.rs               approved exports and ZIP downloads
    manifest.rs              supported-format and provenance validation
    model.rs                 identifiers, JSON access, timestamp/conflict helpers
  engine/                    native MCAP execution, independent of HTTP
    reader.rs                streaming records and channel/schema identities
    decoder.rs               JSON and embedded Protobuf decoding
    images.rs                camera payload and pixel-format decoding
    index.rs                 decoded frame/record cache construction
    translation.rs           field paths and arithmetic translations
    synchronization.rs       clocks, interpolation, resampling
    processing.rs            pipeline execution and new MCAP output
  storage/                   filesystem access and portable folder persistence
    filesystem.rs            containment, atomic writes, fingerprints, hashes
    project_format.rs        descriptors, references, incremental publication
    pages.rs                 immutable bounded JSON pages
    annotations.rs           annotation page and metadata persistence
    datasets.rs              membership/configuration persistence
  analysis.rs                numeric distributions and PCA
  project.rs                 application-facing Project handle

frontend/src/
  App.tsx                    page/component coordination
  pages/                     sign-in, accounts, separate review queue/detail, projects, MCAP list
  components/
    layout/                  navigation, breadcrumbs, page heading
    shared/                  dialogs and empty states
    accounts/                user/group permission editors
    reviews/                 MCAP status bar, working-copy owner, shared assignee picker, submission,
                             review actions, separate revision history, lazy review player
    projects/                folder browser and project members
    mcaps/                   topic table
    annotations/             editor, canvas, inspector, timeline, structured editor, exports
    datasets/                dataset workspace and inspection
    labels/                  shared label/list management
    processing/              translation and synchronization controls
  state/                     authentication and account session context
  hooks/                     workspace, annotation, dataset, playback, revision loading/decisions
  annotations/               pure annotation/track state and pointer-tool behavior
  services/                  HTTP transport and typed workspace data access
  types/                     API models, workspace modes, editor contracts
  utils/                     recording paths, display formatting, topic counts
```

`api/mod.rs` composes routes and middleware; resource handlers delegate to `Store`. `store/mod.rs` owns registration, locks, and validated caches, while domain modules implement the existing operations. `storage/` contains filesystem/layout details without depending on application state. `engine/` owns decoding and processing. The `Store` and engine facades preserve existing Rust imports, so callers do not need to know their internal module layout.

`App.tsx` composes page and domain components. `useWorkspace` owns shared project/recording selection, navigation guards, import actions, processing actions, errors, and notices. `services/workspace.ts` owns those resource requests; `services/http.ts` owns fetch/error handling. Components receive typed props or narrow selections from a controller. `useAnnotationEditor` owns draft/history/save/leave behavior; `annotations/tools.ts` owns pointer and keyframe editing, and separate canvas/timeline/inspector components own rendering. `useDatasetWorkspace` owns dataset loading, source selection, and membership saves. Pure annotation transformations and stable track identities live in `annotations/state.ts`.

Add an endpoint to its resource handler plus the router, an application mutation to its `store/` domain, a file-layout operation to `storage/`, and a decoder/processing capability to `engine/`. Add frontend data access to `services/`, reusable lifecycle behavior to `hooks/`, domain rendering to its component folder, and wire new page modes through the workspace controller. Keep file paths out of UI state. Shared CSS remains in `styles.css` to preserve theme and cascade behavior.

## Development and production

During development, Vite proxies `/api` requests to the Rust backend. `scripts/dev.mjs` supervises both processes, checks for occupied ports, and terminates both process trees on exit. Ports and paths can be configured in `.env`.

In production, the Rust server serves the compiled React files and API from the same origin. `npm run build` builds both parts and `npm start` starts Rust. The backend binds to localhost by default.

## Frontend workflow

The frontend starts at project selection without automatically loading a saved project or its first recording. Navigation follows projects → project MCAP list → MCAP topic table → topic editor. The topic table exposes annotation format, label list, annotation count, review progress, and Edit for each channel. Processing and exports are contextual actions in this view.

The editor registers a navigation guard for all in-app exits. With pending changes, the guard resolves only after Save, Discard, or Cancel. Save failures keep the draft and prompt open. A topic stays selected throughout editing; Back To Table returns to the same MCAP, and Back to project returns to its recording list. Shared labels can be added without replacing the annotation draft.

Dataset creation loads saved annotation summaries from all project recordings, shows annotated and reviewed counts per topic, and permits selecting multiple topics across recordings. Membership uses the existing reviewed-entry API. If a later addition fails, the created dataset and remaining selection are retained for retry. Completed additions remain in the dataset and retries are idempotent.

## Storage

New projects use version 3 folder storage: a compact `project.json` references separate recording, class, label-list, and dataset metadata. Annotation and membership pages cover at most 256 frame numbers per channel. Version 2 projects retain their original layout until explicitly upgraded. Recordings remain in `mcap/<uuid>/`; their bytes and stable UUIDs are preserved. See [project format and PyTorch usage](PROJECT_FORMAT.md) for the layout, migration, incremental behavior, and API.

The Rust store serializes mutations and publishes JSON through temporary files and atomic replacement. Metadata caches track referenced entity files; recording hash verification persists file fingerprints across restarts. Each served recording and frame still resolves inside the selected project. Invalid recording identities, stale hashes, invalid frame indices, and invalid annotation geometry return errors instead of replacing valid annotations. The editor and topic table save changed rows through PATCH, with expected values protecting concurrent edits and compact responses preserving local drafts. Ordinary version 3 saves read and write only touched annotation pages. Preview reads share validated frame indexes and recording identities and do not wait on the mutation lock. Uploads stream into project-local staging files, reuse their streaming SHA-256, and defer image extraction until the first recording open in the web app.

Source recordings are retained. Pipeline processing publishes a new recording with its own identity and workflow configuration. Existing Python source folders and sample projects are reference data and are not part of the Rust runtime.

## API

All endpoints below start with `/api`. Errors use JSON with an `error` message. Project and recording identifiers are stable UUIDs.

| Method     | Path                                                                   | Purpose                                                                    |
| ---------- | ---------------------------------------------------------------------- | -------------------------------------------------------------------------- |
| GET        | `/health`                                                              | Backend availability                                                       |
| GET / POST | `/projects`                                                            | List / create projects                                                     |
| POST       | `/projects/browse`                                                     | Browse local folders and discover project folders without registering them |
| POST       | `/projects/open`                                                       | Open a project by local path                                               |
| GET        | `/projects/{project}`                                                  | Project recordings, labels, lists, and datasets                            |
| POST       | `/projects/{project}/recordings`                                       | Multipart MCAP upload, using the `file` field                              |
| GET        | `/projects/{project}/recordings/{recording}`                           | Indexed topics and saved annotation state                                  |
| GET        | `/projects/{project}/recordings/{recording}/frames/{channel}/{frame}`  | Decoded image                                                              |
| GET        | `/projects/{project}/recordings/{recording}/records/{channel}/{frame}` | Decoded record                                                             |
| PUT        | `/projects/{project}/recordings/{recording}/annotations`               | Validate and save annotation state                                         |
| PUT        | `/projects/{project}/recordings/{recording}/workflow`                  | Save processing settings                                                   |
| POST       | `/projects/{project}/pipeline`                                         | Create a processed recording                                               |
| POST       | `/projects/{project}/labels`                                           | Add a shared class                                                         |
| POST       | `/projects/{project}/label-lists`                                      | Create a named class list                                                  |
| POST       | `/projects/{project}/datasets`                                         | Create a named dataset                                                     |
| GET        | `/projects/{project}/datasets/{dataset}`                               | Dataset membership                                                         |
| POST       | `/projects/{project}/datasets/{dataset}/entries`                       | Add reviewed frames / records                                              |
| POST       | `/projects/{project}/exports`                                          | Export reviewed annotations                                                |
| GET        | `/projects/{project}/exports/{export}/download`                        | Download a self-contained export ZIP                                       |

The folder browser accepts an optional `path` and returns the current folder, parent, subfolders marked with `is_project`, and Workspace/Home/Desktop shortcuts. It lists folders on the backend computer. Opening a selected project remains a separate request.

Image and record indices returned to the browser represent nanosecond timestamps as decimal strings to avoid JavaScript integer precision loss. On-disk indices retain integer timestamps.

## Reference implementations

The original Python applications and tests remain runnable with their existing requirements. Their behavior and additional format support are documented in [PYTHON_REFERENCE.md](PYTHON_REFERENCE.md). The Rust and React implementations can be developed independently of those folders.

Production versions have immutable definitions and membership; generated files are separate materializations. Review snapshots pin bounded source pages. Account identity tombstones keep history independent of active users. See [the complete workflow](ACCOUNTS_AND_REVIEWS.md).
