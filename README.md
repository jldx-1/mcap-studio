![MCAP Studio — Record. Refine. Release.](docs/assets/mcap-studio-banner.svg)

# MCAP Studio

MCAP Studio is a shared workspace for turning MCAP sensor recordings into datasets for **AI model training**. Its main purpose is to give teams a quick, easy data preparation pipeline: import recordings, process and synchronize sensor streams, annotate data, review labels, and export reproducible training datasets.

It is intended to run on a **local network, with remote teammates connecting through a VPN**. One host runs the application and stores the project files; team members use their browsers to work with the same projects, permissions, and review queue.

## How to run

Install [Rust](https://www.rust-lang.org/tools/install) **1.89+** and Node.js **20.19+ or 22.12+**. From the repository root:

```bash
npm run setup
npm run dev
```

Open **http://127.0.0.1:5173** to use the app locally. First launch lets you create the administrator account. The Rust API runs on **http://127.0.0.1:8000**.

The dev command starts the backend first and waits for its health endpoint before launching the frontend. The first Rust build takes longer while dependencies compile. Ctrl+C stops both services. Frontend edits reload automatically; restart after Rust changes.

If you have [just](https://just.systems/) installed, the equivalent commands are:

```bash
just setup
just dev
```

Python and Tk are not required to run the new application.

### Large recordings

MCAP uploads stream to disk. Set `MCAP_UPLOAD_LIMIT_GIB` in `.env` to control the maximum size of each MCAP file (default: 64 GiB; `0` means unlimited). The limit excludes multipart overhead. Restart the backend after changing it. Oversized uploads return HTTP 413 and discard their staged files. Import defers preview generation until the recording is opened or prepared. Processing without synchronization streams messages in source order with bounded payload memory; synchronization still buffers decoded samples and can require substantial RAM. Preview images use fast lossless PNG compression, and development commands run the optimized Rust backend (the first build takes longer).

Camera indexing keeps Protobuf image payloads binary, decodes each payload once, and uses direct byte conversion for 8-bit images. Training cache completion markers avoid rescanning every frame on repeated dataset builds. To measure preview throughput on this machine, run `cargo test --manifest-path backend/Cargo.toml --locked --release --test index_performance -- --ignored --nocapture` (fixture generation is excluded from timing).

Video playback in labeling and review follows MCAP frame timestamps at 1× speed, including variable frame intervals and long gaps. A small, bounded prefetch buffer reduces loading stalls. When loading cannot keep up, playback buffers rather than skipping frames or accelerating through them. Timing regression checks: `node scripts/playback-timing-smoke.mjs`.

Uploads use a bounded background writer that buffers disk I/O and hashes received bytes in the same pass. Processed recordings compute their SHA-256 during preview indexing instead of rereading the MCAP separately. Dataset builds reuse compiled sensor translations and buffered record reads; YOLO builds omit unused CSV serialization. Native tensors use row copies where possible and reuse validated outputs on repair/retry, rebuilding if source metadata, bytes, conversion mode, or output fingerprints change.

The sensor-window benchmark compares the former read/translate loop with the buffered path: `cargo test --manifest-path backend/Cargo.toml --locked --release --lib sensor_window_throughput -- --ignored --nocapture`.

For multi-gigabyte recordings, use SSD storage. On WSL, a workspace in the Linux filesystem avoids the overhead of creating frame caches on `/mnt/c`. Files already on the server can be imported from the recording library without a browser upload. `npm run build` followed by `npm start` serves the app directly on port 8000, avoiding the development proxy. Any additional reverse proxy must allow the desired upload size and duration.


## Team access over a local network and VPN

For a shared team workspace, run the built app on a host reachable from your local network and VPN. On that host:

1. Run `npm run setup` if dependencies are not installed.
2. Copy `.env.example` to `.env` and set the following values. Replace the workspace path with an absolute directory on the host:

   ```dotenv
   MCAP_HOST=0.0.0.0
   MCAP_BACKEND_PORT=8000
   MCAP_WORKSPACE=/absolute/path/to/team-workspace
   ```

3. Build and start the application:

   ```bash
   npm run build
   npm start
   ```

4. Open **http://HOST-IP:8000**, replacing `HOST-IP` with the host's reachable LAN or VPN address. The same port serves the frontend and API. Create the administrator account, then add users/groups and assign project access.

Remote teammates connect to your team's VPN before opening that address. Restrict inbound access to the LAN/VPN using the host's firewall; `0.0.0.0` listens on all IPv4 interfaces. VPN setup is managed separately from MCAP Studio. Project files and generated datasets stay on the host, so choose a persistent workspace with enough storage for recordings and frame caches.

### Browser and server file locations

The browser can run on a different computer from the backend. **Browse server** and **Load from server** list folders and files on the backend host. Paths such as `/data/projects` or `C:\recordings` refer to that server; they do not open folders on the user's device. **Load MCAPs**, drag-and-drop, and YOLO file/folder selection read files on the user's device and upload their contents to the server. A local project folder must be copied to server storage before it can be opened as a project.

The built app served by Rust works for remote browsers without additional frontend configuration. To run the development frontend on a different host, set `MCAP_API_PROXY_TARGET=http://BACKEND-HOST:8000` in that frontend checkout's root `.env`, then run `npm --prefix frontend run dev -- --host 0.0.0.0`. On the backend, listen on a reachable address and include the frontend's browser-facing origin (for example `http://FRONTEND-HOST:5173`) in `MCAP_ALLOWED_ORIGINS`.

For separate production hosting, serve `frontend/dist` from the web host and reverse-proxy **all `/api/*` requests**, including image previews, uploads, and downloads, to the backend. Keep `/api` under the frontend's public origin so session cookies work with the existing same-origin clients. Preserve the public `Host` header when proxying, or explicitly allow the frontend origin through `MCAP_ALLOWED_ORIGINS`. Allow the configured upload size and duration in the proxy too. For public HTTPS hosting, enable `MCAP_SECURE_COOKIES=1` on the backend. The frontend's `127.0.0.1` development defaults are not remote browser API addresses.

## Repository layout

```text
mcap-studio/
  frontend/                 # React + TypeScript browser application
  backend/                  # Rust API, MCAP processing, accounts, and storage
  python/                   # dataset access package and optional PyTorch loaders
  scripts/                  # setup, development, build, and smoke checks
  docs/                     # architecture, storage, reviews, and Python reference
    assets/                 # README banner artwork
  data/                     # local runtime data; ignored by Git
    projects/               # new projects in the default workspace
    cache/                  # decoded recording caches
    accounts/               # users, groups, and sessions
  package.json              # npm commands for both services
  justfile                  # optional command shortcuts
  .env.example              # host, ports, and workspace settings
```

## Data preparation workflow

1. **Sign in and choose a project.** First launch creates an administrator. Use the sidebar account menu for profile, permissions, admin controls, and sign out. Administrators assign users/groups to projects.
2. **Open an MCAP and annotate.** Drag one or more `.mcap` files anywhere into the workspace, or use **Load MCAPs**. With a project open, files are added to that project; otherwise, open or create a project to import the dropped files. Choose topics and label lists, and save working-copy annotations or CSV labels. The Recordings page provides a drop area and a searchable table with topic count, recording length, and MCAP file size. Choose 10, 20, 30, 40, or 50 rows per page. Recording metrics do not require camera frames or preview indexing; playback uses the decoded cache.
3. **Submit a revision.** Each submission creates an immutable snapshot. The working copy can continue changing without altering older revisions.
4. **Review the snapshot.** Recordings shows compact review-status links. The notification bell shows your review requests, returned feedback, and background job status. The separate Reviews queue opens a dedicated revision page with playback, previous feedback, and general or timestamp/frame references. Approve revision accepts the complete submission immediately; Submit feedback returns it to Feedback received. Previous snapshots, decisions, and comparisons remain available.
5. **Browse approved data.** Filter approved labels, modalities, projects, sources, and people. Associated sensor streams can be included as synchronized inputs.
6. **Prepare AI training data.** Create a dataset version with exact approved revisions, labels, modalities, and synchronization. Build CSV/YOLO outputs or use lazy PyTorch loaders with those pinned inputs. New approvals require an explicit new dataset version, making training inputs reproducible.

Project content remains ordinary files. Version 3 uses small metadata and bounded immutable pages; existing source files remain in place. MCAP bytes are never rewritten for account, annotation, review, or dataset metadata changes. See [accounts, immutable reviews, approved data, and training versions](docs/ACCOUNTS_AND_REVIEWS.md) and [project storage](docs/PROJECT_FORMAT.md).

The **Datasets** section opens **My datasets**, grouped into approved dataset versions, imported YOLO datasets, and working label tables. **Choose recordings** lets you select recordings; expand **Choose exact approved revisions and filter labels or modalities** to see one card per recording/revision; expand **Topics and sample counts** to inspect labelled streams and synchronized inputs. Label, modality, and reviewer filters are available under the expandable filter controls.

Use **Datasets → Import YOLO** to load a ZIP, a dataset folder, or `.txt` label files together with their PNG/JPEG frames. Matching paths such as `images/train/frame.png` and `labels/train/frame.txt` retain their train/validation/test splits. Imports support the [YOLO detection format](https://docs.ultralytics.com/datasets/detect/): one `class x_center y_center width height` row per box, with normalized coordinates. Class names come from `data.yaml`, `classes.txt`, `obj.names`, or the optional class-name input. Images without label files are included as negative examples. Imported datasets have frame previews with bounding boxes and a **Download YOLO ZIP** action.

## Native format support

The Rust backend reads JSON and embedded Protobuf schemas. Camera decoding supports PNG/JPEG and these raw image formats, including row stride and endianness. Encodings are case-insensitive; unsupported encodings are identified in the topic list.

| Image layout | Supported encodings |
| --- | --- |
| Grayscale integers | `mono8`, `mono10`, `mono12`, `mono14`, `mono16` (also `mono_8` through `mono_16` for those bit depths), `8UC1`, `16UC1` |
| Integer color | `rgb8`, `bgr8`, `rgba8`, `bgra8`, their `16`-bit equivalents, `8UC3`, `8UC4`, `16UC3`, `16UC4` |
| Float grayscale | `32FC1`, `FP32C1`, `mono32f`, `64FC1`, `FP64C1`, `mono64f` |
| Float color | `32FC3`, `FP32C3`, `32FC4`, `FP32C4`, their `64`-bit equivalents, and `rgb32f`, `bgr32f`, `rgba32f`, `bgra32f` (also `64f`) |

`mono10/12/14` use right-aligned unsigned samples in 16-bit containers, scaled against their bit-depth maximum (1023/4095/16383). Bit-packed layouts require conversion before import. Generic three/four-channel encodings use BGR/BGRA order; use explicit `rgb*`/`rgba*` names for RGB/RGBA input. Alpha is omitted from display previews and retained in native training tensors.

Float frames normalized to `[0, 1]` map directly to display intensities `[0, 255]`. Other float ranges use the finite minimum and maximum of each frame, shared across all displayed color channels, excluding alpha and row padding. NaN/infinite samples display as black; constant frames outside `[0, 1]` also display as black. This scaling applies only to visualization: native training tensors retain the recorded values, with color tensors exported in RGB/RGBA order. Existing recordings with cached unsupported raw-format errors rebuild their previews automatically when opened after this decoder update.

Use **Labels & Translations** to manage label lists and topic translation scripts together. Select a recording, then select a saved preset or choose **Create or load** beside a topic. The script editor expands beneath that topic’s row, where you can create a named script, load a legacy translation JSON file or a plain `.txt` script, and see the current recorded sample, translation script, and output side by side. Output updates automatically as you edit; invalid scripts show an error while keeping the input visible. Saved presets match the topic, message type, and schema fingerprint; a single matching native preset is applied automatically when a new MCAP is imported. The recording library shows a single entry per source; multiple matching presets require a manual choice. If a script fails, the recording remains available with an explanatory notice. **Apply selected translations** opens the processed topic table, reusing an existing output when the scripts are unchanged. Untouched topics retain their native schemas and message payloads; translations stream through selected topics without decoding the other streams.

Reusable files are saved under the project's `translations/library/` folder. Legacy version-1 files from `utils/translations/`, `utils/mcap-data-pipeline/translations/`, and the preserved `misc/mcap-data-pipeline/translations/` directory are also discovered. Legacy `script` text and `mappings` arrays are accepted, with Protobuf schema fingerprints normalized by descriptor file order.

Field translations support dotted paths, array/map indexing, copying, renaming, numeric arithmetic, and `abs`, `round`, `min`, `max`, and `sqrt`:

```text
* -> *
position.x -> position_mm.x * 1000
temperature_c <- (source.temperature_f - 32) * 5 / 9
peak <- max(source.samples)
first_x <- source.entities[0].position.x
```

Synchronization supports nearest samples, linear interpolation, and rate extrapolation against a parent channel. Empty tolerance means unlimited; a numeric tolerance limits the nearest source distance. Processing settings are saved with the recording.

The web app exports approved data as COCO, YOLO, and JSONL. Video packets, Bayer/YUV camera decoding, Protobuf output descriptors for translations, and the additional Python export formats remain available through the preserved Python applications. YOLO imports currently support detection boxes. Polygon, mask, and skeleton editing are not implemented.

## Configuration

Copy `.env.example` to `.env` to change the host, ports, or workspace. Both npm and just commands load this file:

```dotenv
MCAP_HOST=127.0.0.1
MCAP_BACKEND_PORT=8000
MCAP_FRONTEND_PORT=5173
# MCAP_WORKSPACE=/absolute/path/to/projects-parent
```

By default, the backend discovers projects in this repository and its `data/projects/` directory. New projects are created under `data/projects/`. Opening a project by path registers it for the backend. Uploaded MCAPs are copied into that project's recording folder.

Accounts use server-managed sessions, configurable groups, and project membership. See [account setup, permissions, and review workflow](docs/ACCOUNTS_AND_REVIEWS.md). The backend binds to localhost by default; use the team access instructions above for LAN/VPN hosting.

## Commands

| Command                        | Purpose                                                                 |
| ------------------------------ | ----------------------------------------------------------------------- |
| `npm run setup` / `just setup` | Install the frontend and fetch Rust dependencies                        |
| `npm run dev` / `just dev`     | Start both development services                                         |
| `just backend`                 | Start only the Rust API                                                 |
| `just frontend`                | Start only the React development server                                 |
| `npm run check` / `just check` | Rust formatting, Clippy, tests, TypeScript checking, and frontend build |
| `npm run build` / `just build` | Build React assets and an optimized Rust executable                     |
| `npm start` / `just start`     | Serve the built frontend and API together from Rust on port 8000        |
| `just python-test`             | Run pytest from the repository root                                     |

Without just, run individual services with `cargo run --manifest-path backend/Cargo.toml --locked` and `npm --prefix frontend run dev`.

The production command requires `npm run build` first. The Rust server serves `frontend/dist/`, with an SPA fallback for application routes. Override that directory with `MCAP_FRONTEND_DIR` if moving the executable.

For an API integration check against the synthetic fixture, start the backend using a temporary workspace, then run the smoke check in a second terminal:

```bash
MCAP_WORKSPACE=/tmp/mcap-studio-smoke cargo run --manifest-path backend/Cargo.toml --locked
npm run smoke -- misc/synthetic_robot_arm.mcap
```

The smoke check requires a fresh isolated workspace, creates administrator/reviewer accounts and a new test project and exercises import, previews, annotation integrity, approved datasets, exports, and processing. The synthetic recording is ignored by Git; see [synthetic fixture generation](misc/SYNTHETIC_MCAP_README.md) to create it locally.

The older full-workspace browser smoke check in `scripts/frontend-smoke.mjs` uses Playwright and two distinct fixture MCAPs with `/camera` image and `/motor` record topics. Run it against an isolated workspace after building the frontend:

```bash
MCAP_UI_URL=http://127.0.0.1:8000 node scripts/frontend-smoke.mjs /path/to/session-a.mcap /path/to/session-b.mcap
```

Install Playwright separately for this check, or set `MCAP_PLAYWRIGHT_MODULE` to an existing installation. It verifies project browsing, MCAP imports, topic settings, draft preservation, save/discard/cancel and failure recovery, reviewed dataset selection across recordings, idempotent additions, dataset loading, and mobile layout.

See [architecture, module map, and API](docs/ARCHITECTURE.md) for integration details and [Python reference](docs/PYTHON_REFERENCE.md) for the preserved desktop applications.

The current account/review browser check is `scripts/review-smoke.mjs`; playback/import regression checks are in `scripts/playback-smoke.mjs`. Both use isolated test workspaces. See [accounts and reviews](docs/ACCOUNTS_AND_REVIEWS.md#verification).
