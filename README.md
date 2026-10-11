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

The first Rust build takes longer while dependencies compile. Ctrl+C stops both services. Frontend edits reload automatically; restart after Rust changes.

If you have [just](https://just.systems/) installed, the equivalent commands are:

```bash
just setup
just dev
```

Python and Tk are not required to run the new application.

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
2. **Open an MCAP and annotate.** Load recordings, choose topics and label lists, and save working-copy annotations or CSV labels. Imports keep the current section and playback uses the decoded cache.
3. **Submit a revision.** Each submission creates an immutable snapshot. The working copy can continue changing without altering older revisions.
4. **Review the snapshot.** Project MCAPs shows compact review-status links. The separate Reviews queue opens a dedicated revision page with full-video playback, previous feedback, and frame-range/general comments. Approve Revision accepts the complete submission immediately; Request Changes sends it back with feedback. Previous snapshots, decisions, and comparisons remain available.
5. **Browse approved data.** Filter approved labels, modalities, projects, sources, and people. Associated sensor streams can be included as synchronized inputs.
6. **Prepare AI training data.** Create a dataset version with exact approved revisions, labels, modalities, and synchronization. Build CSV/YOLO outputs or use lazy PyTorch loaders with those pinned inputs. New approvals require an explicit new dataset version, making training inputs reproducible.

Project content remains ordinary files. Version 3 uses small metadata and bounded immutable pages; existing source files remain in place. MCAP bytes are never rewritten for account, annotation, review, or dataset metadata changes. See [accounts, immutable reviews, approved data, and training versions](docs/ACCOUNTS_AND_REVIEWS.md) and [project storage](docs/PROJECT_FORMAT.md).

## Native format support

The Rust backend reads JSON and embedded Protobuf schemas. Camera decoding supports PNG/JPEG, raw RGB/BGR/RGBA/BGRA at 8/16 bits, and mono at 8/16 bits or 32/64-bit float, including row stride and endianness. Unsupported encodings are identified in the topic list.

Field translations support dotted paths, copying, renaming, and numeric arithmetic:

```text
* -> *
position.x -> position_mm.x * 1000
temperature_c <- (source.temperature_f - 32) * 5 / 9
```

Synchronization supports nearest samples, linear interpolation, and rate extrapolation against a parent channel. Empty tolerance means unlimited; a numeric tolerance limits the nearest source distance. Processing settings are saved with the recording.

The web app exports approved data as COCO, YOLO, and JSONL. Video packets, Bayer/YUV camera decoding, Python expression functions/array indexing, and the additional Python export formats remain available through the preserved Python applications. Polygon, mask, and skeleton editing are not implemented.

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
