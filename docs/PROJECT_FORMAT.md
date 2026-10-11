# Incremental project folders

New projects use format version 3. JSON files, ordinary MCAP recordings, PNG images, CSV files, and YOLO text labels remain directly accessible. There is no database or project container.

```text
my_project/
  project.json                         # identity and entity references only
  mcap/<recording-uuid>/
    recording.mcap                     # source; annotation/configuration edits never write it
    metadata.json                      # source identity, hash, provenance, configuration references
    web-workflow.json                  # selected topics and workflow defaults
    frames/                            # rebuildable decoded images, JSONL records, timestamp index
  labels/<class-uuid>/label.json        # shared class definitions
  labels/lists/<list-uuid>.json         # ordered lists of stable class UUIDs
  annotations/<recording-uuid>/
    metadata.json                      # classes, settings, tracks, techniques, page references
    pages/<channel>/<bucket>-<hash>.json
  datasets/<dataset-uuid>/
    dataset.json                       # source configuration, counts, sources, membership pages
    entries/<recording>/<channel>/<bucket>-<hash>.json
    data.csv                           # structured dataset CSV, when applicable
    generated/csv/                      # rebuildable CSV and lazy sample pages
    generated/yolo/                     # rebuildable images/, labels/, data.yaml, sample pages
  translations/<recording-uuid>.json    # source topic mappings
  synchronization/<recording-uuid>.json # source synchronization rules
  indexes/<recording-uuid>/             # rebuildable fingerprints and project summaries
  cache/                               # reserved for additional rebuildable artifacts
  exports/                             # self-contained, explicit export snapshots
```

The existing `mcap` directory name is retained to preserve recording paths. Projects created with the application initially contain configuration directories; saved workflow mappings populate their files. Users may keep synchronization/translation scripts alongside configurations, but the Rust application executes declarative configuration only.

## Source and derived data

`project.json`, entity metadata, label definitions, annotation pages, dataset membership/configuration, translations, and synchronization rules are the source of truth. Keep these files when clearing caches. Dataset membership is an intentional selection and cannot be inferred from MCAP bytes alone.

Decoded `frames/`, `indexes/`, `cache/`, dataset `generated/` outputs, and exports are derived artifacts. Removing a complete frame cache causes the application to decode that recording when next needed. Removing a generated dataset allows Build to recreate it from source membership and annotations. CSV column definitions and row overrides remain in `dataset.json`.

Each annotation or membership page covers at most 256 frame numbers from one recording/channel. Page filenames include a SHA-256 fingerprint of their canonical JSON contents. An update writes changed pages first, then atomically replaces their small metadata document. Unchanged pages and unrelated entities keep their contents and modification times. Superseded immutable pages are retained so a reader holding an earlier metadata snapshot can finish; offline cleanup may delete pages no longer referenced by any source/generated metadata. No automatic source-page deletion runs during editing.

Edit annotations through the project API. If editing a page directly, publish a new page with its recomputed hash and update the metadata reference; the backend rejects mismatched source hashes. Label and configuration JSON files can be edited directly, and file fingerprints invalidate their metadata cache. Avoid external edits during application mutations; native writers coordinate through `.project.lock`. Files are atomically replaced individually; multi-entity mutations are not a database transaction.

The web app stages uploads directly inside the project and hashes incoming bytes once. Saving an MCAP no longer waits for decoding every image; the first recording open builds its frame cache once. API clients may opt into this behavior with `?defer_index=true`; the response includes `indexing: "pending"`. Default API uploads and `Store::import_recording` still return fully indexed recording details.

## Incremental behavior

| Change | Files affected |
| --- | --- |
| Rename a class | Its `label.json`; a subsequent dataset build updates class names |
| Edit an annotation | Its affected 256-frame pages and that recording's annotation metadata |
| Reorder a label list | That list's metadata; stable annotation class identities remain intact |
| Change workflow mappings | Changed translation/synchronization/workflow JSON and recording metadata if references change |
| Add dataset membership | Changed membership pages and that dataset's metadata |
| Repeat a dataset build | No output writes if inputs and outputs are unchanged |
| Edit one included annotation and rebuild | Its generated sample page and YOLO label; existing image bytes are reused |
| Revoke review and rebuild | Remove that sample from generated membership and its YOLO image/label |
| Modify a recording externally | Recompute its SHA-256 and reject stale identities; import it as a new recording |

Opening a project reads descriptors and cached summaries, without hashing or decoding every MCAP. Summaries use frame-index and annotation-metadata fingerprints. Recording SHA-256 verification is persisted with size, nanosecond modification time, and platform file identity (plus Unix change time), then reused while that fingerprint is unchanged. Initial import and missing-cache rebuilds still scan the requested recording. MCAP reads stream records/chunks rather than allocating the entire recording as one byte buffer.

Dataset Build fingerprints source membership, recording identity, the included annotation, class-number mapping, relevant topic translation, shared synchronization rules, and builder version. It checks output existence and image fingerprints, reuses extracted images, and writes only changed sample pages/text. Shared synchronization changes affect all samples from that recording. A translation for an unrelated topic leaves the selected topic's samples unchanged. Translation changes invalidate dependent sample metadata; transforming raw messages is still an explicit pipeline action producing a new recording.

Build performs a pass over the selected dataset's membership and metadata. It does not rescan unchanged MCAP bytes. A flat CSV has one header/body, so a changed CSV necessarily replaces that dataset's CSV file; its source/sample pages remain incremental. The editor initially receives the selected recording's annotation state and frame index. Subsequent ordinary annotation saves validate and read only touched 256-frame pages and metadata; changing class identities, formats, or label associations performs full validation. Frame previews use shared indexes and recording fingerprints, without reading annotations or taking the mutation lock. Synchronization processing retains decoded selected streams for interpolation. These costs are proportional to the selected data, rather than every recording in the project.

## Application API

Rust consumers can use `mcap_workbench::Project`:

```rust
let project = mcap_workbench::Project::open("./my_project")?;
let recordings = project.mcaps()?;
let labels = project.labels()?;
let dataset = project.dataset("robot_detection_v1")?;
project.build_dataset(dataset["uuid"].as_str().unwrap(), "yolo")?;
```

The handle also provides `annotations`, `add_annotation`, `update_annotations`, `update_label`, `create_dataset`, and `datasets`. `Store` exposes the complete application operations, including import, workflow changes, membership selection, and structured CSV creation.

HTTP additions, all under `/api`:

| Method | Path | Request |
| --- | --- | --- |
| POST | `/projects/{project}/recordings?defer_index=true` | Multipart `file`; save without extracting images yet |
| PATCH | `/projects/{project}/recordings/{recording}/annotations` | `sha256`, `changes`, optional `metadata`/`expected_metadata` |
| PUT | `/projects/{project}/labels/{label}` | `name`, optional `expected_name` |
| POST | `/projects/{project}/datasets/{dataset}/build` | `{"format":"csv"}` or `{"format":"yolo"}` |
| POST | `/projects/{project}/upgrade` | No body required |

An annotation change is `{"channel_id":"1","frame":42,"annotation":{...},"expected":{...}}`. Use `null` to add a previously absent row (`expected`) or delete a row (`annotation`). Expected values reject concurrent overwrites. The web editor and topic table submit changed rows and metadata through PATCH with `return_delta: true`. The response contains `metadata` and approved `changes`, which the client applies to its local draft. Omitting `return_delta` retains the complete-state response. The full-state PUT endpoint remains supported for existing clients.

Create image membership using the existing dataset and entries endpoints, then choose **Build YOLO** or **Build CSV** in the dataset view. Create structured CSV datasets through the topic's column workflow, then choose **Build CSV**. Generated CSV image datasets include project-relative image paths and annotation JSON. YOLO detection emits normalized boxes, with approved empty frames represented by empty label files. Classification labels require CSV; they cannot silently become empty YOLO detection targets. YOLO output describes the complete selected image set; supply a separate validation split for training as needed.

Adding another exporter requires an output implementation in `backend/src/store/dataset_generation.rs` and, if its sample type differs, a Python adapter. Source membership and annotation storage do not depend on the output format.

## PyTorch

Install the small Python library and optional training dependencies:

```bash
python -m pip install -e './python[torch]'
```

Build the dataset in the app first. Loading requires no running backend:

```python
from mcap_project import Project, detection_collate
from torch.utils.data import DataLoader

project = Project.open("./my_project")
dataset = project.dataset("robot_detection_v1").to_pytorch()
loader = DataLoader(dataset, batch_size=32, shuffle=True,
                    num_workers=4, collate_fn=detection_collate)
```

Image samples are `(image, target)`: RGB float tensors in `[0, 1]`, pixel `xyxy` boxes, integer class IDs, classifications, and exact timestamps. `detection_collate` handles different image sizes and box counts. Class IDs use the generated manifest's ordered `classes`/`class_uuids` definitions.

For tabular sensor data:

```python
dataset = project.dataset("motor_training").to_pytorch(
    feature_columns=["position", "velocity"], target_column="fault"
)
loader = DataLoader(dataset, batch_size=32, shuffle=True)
```

Numeric feature columns are selected automatically if omitted; mixed/text columns require an explicit selection or transformation. Synchronized sensor values are loaded from the columns produced by the pipeline and dataset workflow. Loading does not perform additional sensor alignment. An optional `transform` receives the image/target pair or tabular result.

The loader reads only the sample-page directory at construction, caches at most eight 256-frame pages per worker, and opens images only on access. It is picklable and supports multiworker DataLoaders. Generated datasets are training snapshots: finish Build before training, and rebuild/reopen after source changes. The Python interface provides read access; application mutations use the Rust/HTTP project API. Its map-style implementation follows [PyTorch's Dataset and DataLoader interface](https://docs.pytorch.org/docs/stable/data.html).

## Existing projects and validation

Version 2 projects remain readable/editable in their original layout. Call the upgrade endpoint to convert metadata in place; it saves `project.v2.backup.json`, retains UUIDs and recording/frame-cache paths, and never rewrites MCAP bytes. Older Python reference apps support versions 1/2; use the Rust app or this library after upgrading. Version 1 remains read-only.

```bash
cargo test --manifest-path backend/Cargo.toml --locked
cargo clippy --manifest-path backend/Cargo.toml --locked --all-targets -- -D warnings
PYTHONPATH=python python3 -m unittest discover -s python/tests -v
```

Tests cover deferred imports, compact saves that do not read unrelated pages, source-file preservation, changed/unchanged pages, no-op builds, class renaming, topic-scoped invalidation, negative frames, CSV regeneration, migration, external modifications, traversal/symlinks, loader page boundaries, bounded caches, and real PyTorch workers.

The browser playback regression uses an isolated backend and an MCAP with at least three image frames:

```bash
MCAP_UI_URL=http://127.0.0.1:8000 node scripts/playback-smoke.mjs /path/to/recording.mcap
```

Install Playwright separately or provide `MCAP_PLAYWRIGHT_MODULE`. The check delays frame responses, disables HTTP caching, verifies actual displayed image URLs, exercises pause/Space/replay, and saves annotations through the editor. Playback downloads and decodes each next image before advancing, retaining only the current displayed image and next pending frame. Recorded gaps are capped at one second for preview playback.

Account and task-review metadata are stored separately from MCAP and annotation pages. See [accounts, project access, review snapshots, and approval-gated generation](ACCOUNTS_AND_REVIEWS.md).

Production datasets now pin immutable approved annotation revisions. Their version definitions, snapshot references, bounded membership pages, sensor recipes, and PyTorch interface are documented in [accounts and reviews](ACCOUNTS_AND_REVIEWS.md). Legacy live membership remains separate from production versions.
