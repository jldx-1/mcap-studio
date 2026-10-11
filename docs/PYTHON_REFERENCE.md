# Python reference: MCAP pipeline and local labeling

This is the preserved documentation for the Python desktop applications in
`misc/`. Run the commands below from that directory (`cd misc` from the
repository root). For the Rust/React application and team access over a VPN,
see the [main README](../README.md).

The pipeline translates and synchronizes MCAP topics. The desktop labeler groups
multiple recordings into named projects and provides a local annotation workflow:
draw/edit bounding boxes, assign classes and track IDs, navigate frames, and
keep tracks visible until exit and interpolate box positions between keyframes. All editing,
image decoding, saving and dataset export happen locally. No CVAT server, account,
browser editor or SDK is required.

```bash
python -m pip install -r requirements.txt
python mcap-data-pipeline/main.py
python mcap-data-labeler/main.py
```

Python 3.10+ and Tk are required (`python3-tk` on many Linux systems).

## Projects and pipeline handoff

1. In the pipeline, choose topics, translations and synchronization settings.
   **Select all topics** checks every topic; **Clear selection** unchecks them.
   In **4. Data Labeling**, choose an annotation format for each topic and
   optionally enter shared project label names.
2. Finish with one of two actions:
   - **Export project…** prompts for a project name and destination parent folder,
     then creates a portable project folder named after the project containing the processed MCAP,
     configuration, reusable workflow and ETL scripts.
   - **Save MCAP to a project…** opens a project chooser showing names, recording
     counts and UUIDs. Browse to a projects parent folder or a project itself,
     select the project, and append the processed MCAP. Existing recordings and
     annotations are retained.
3. **Open in labeler →** opens the recording just saved. You can also run:
   `python mcap-data-labeler/main.py --project /path/to/project-name`
   with optional `--mcap MCAP_UUID` to select a particular recording.
4. In the labeler, use **New project…**, **Open project…** and **Add MCAPs…** to
   manage projects, then choose a recording from the **MCAP** dropdown.
   Each recording has its own frame cache, annotations and saved frame positions.
   Project controls are hidden while editing a topic and return with **Back**.

Projects use their names as folder names and retain stable UUIDs in `project.json`.
Duplicate names in the same parent directory are rejected without overwriting
existing work. Copy or move the complete project folder to move its recordings,
labels and ETL scripts. Existing UUID-named folders remain readable. Version 1 projects are upgraded when opened in the labeler or appended
to; their original MCAP, cache, annotations and manifest are preserved.

## Local annotation workflow

The topic pane fills the window and lists every topic with a labeling format
dropdown, a label list and separate **Label lists** and **Edit** action columns. Choose a format and click **Edit** to
apply it and open the workspace. Box formats can be changed without losing boxes;
switching annotated topics to a different kind of annotation is prevented.
Use **Label lists…** to **Create list**, then **Add label**, **Add existing**, or
import TXT/JSON labels inside it. Assign a named list using the topic dropdown
or **Apply to topic** in the dialog. Lists are shared across recordings, and
new labels become available to every topic using that list. Custom selections
remain available under **All project labels**. Labels already used by a topic
must remain associated. Shared UUIDs preserve numeric annotation IDs when names
change or recordings reopen.

The workspace has three resizable panes: preview on the left, a timeline below
it, and a work panel on the right.

- Drag on empty space to draw. Click a box to select it, drag it to move, or drag
  a corner to resize. There is no draw/select mode switch.
- A new box persists from its entry frame through the recording end. Adjusting
  it on a later frame creates a keyframe; intermediate boxes interpolate using
  actual frame timestamps. After the last keyframe, the box keeps its position.
- The work panel edits box coordinates and class, sets or removes keyframes,
  and marks the last visible frame using **End selected track here** or an explicit
  last visible frame. The box disappears on the next frame. **Keep visible to
  recording end** clears the exit. Setting an earlier
  exit removes later keyframes; Undo restores them.
- Blue marks entry, green marks continued visibility, and purple marks exit.
  The timeline shows each track's duration and keyframe diamonds. Click a track
  to select it and scrub time; double-click a keyframe in the work panel to jump.
- Previous/next frames, configurable step size, frame-number entry and a scrubber
  support navigation. Left/right arrow keys use the Step size throughout the
  editor, preserving normal text and dropdown editing. Delete on the preview
  removes the selected track. Ctrl+S saves and Ctrl+Z undoes the whole track edit.
- **Play / Pause** plays camera frames at their recorded timestamps, advancing
  every frame independently of Step. Playback stops at the final frame; pressing
  Play there restarts it. Editing or manual navigation pauses playback.
- Local foreground suggestions require acceptance before saving. Decoded records
  support classification and JSON annotations. Changes and frame positions autosave.
- The right panel separates **Track**, **Box & keyframes**, and **Assist** controls.
  **Undo**, **Save**, **Export annotations…**, and **Back** sit at the bottom.
  **Back** returns to the topic list and prompts to save, discard, or cancel when
  classification, JSON, box, track exit, or Step edits are pending. **Save** also
  applies those pending edits. Completed annotation changes still autosave.

**Export annotations…** creates a self-contained dataset under the project's
`exports/` folder. Image formats include YOLO, COCO, Pascal VOC, KITTI, MOT and
CVAT XML; classification and record annotations use JSONL. CVAT XML is only an
export file format. Only reviewed frames are exported; reviewed empty frames
represent negatives. MOT requires every frame reviewed. Exports retain the
project UUID, recording UUID, MCAP hash and frame timestamps.

The separate **Datasets** section holds named collections of labeled data. Use
**Load dataset…** to choose a saved dataset from the current project, or
**Create dataset…** to start a new one. Selecting a dataset shows the MCAP
filenames it contains and the frame/record count for each MCAP, without displaying
JSON. Empty datasets show a message until data is added.
Use **Add reviewed data…** to add reviewed
frames/records from every labeled topic in the current recording. Inside the
editor, **Add to dataset…** saves pending edits and adds reviewed data from that
topic. Choose the same dataset while opening other recordings to combine them.
Reviewed empty frames are included as negatives, unreviewed data is excluded,
and adding the same recording/channel/frame again avoids duplicates. The table
shows frame/record and MCAP counts; the MCAP list updates after each addition.

Datasets are stored in `project.json` as a UUID-keyed `datasets` dictionary.
Each dataset has a `name` and an `entries` dictionary keyed by
`<mcap-uuid>:<channel-id>:<zero-based-frame>`. Each entry includes `mcap_uuid`,
`channel_id`, `frame`, `time_ns`, and two references:

- `labeled_data`: the project-relative `annotations.json` path, channel ID, and
  frame number locating the annotation in `annotations[channel_id][str(frame)]`.
  The same file contains the classes and shared class UUIDs.
- `raw_data`: the project-relative MCAP path and hash, frame index path, and either
  an image path or a `records.jsonl` path and byte offset for decoded records.

These collections reference existing files without copying them. Membership
changes when you add data; later saved label edits are visible through the
references. Move the complete project folder to preserve the references.
**Export annotations…** continues to export the current recording's reviewed
annotations independently of named dataset membership.

The current image tools annotate bounding boxes. Polygon, mask and skeleton
editing are not implemented.

## Project dictionary and folders

```text
<parent>/<project-name>/
  project.json                       # project name and UUID registries
  mcap/<mcap-uuid>/
    recording.mcap                   # imported or processed MCAP
    configuration.json               # processed-recording provenance
    workflow.json                    # reusable workflow, when unambiguous
    annotations.json                 # local per-topic labels and editor settings
    frames/
      index.json
      channel_<id>/<frame>.png
  labels/<label-uuid>/label.json      # shared label name/definition
  translations/<mcap-uuid>.json
  synchronization/<mcap-uuid>.json
  scripts/
    translate.py
    synchronize.py
    runtime/                         # portable copies of the ETL implementation
  exports/<export-uuid>/
    annotations.json
    classes.json
    manifest.json                    # project/recording identity and export counts
    channel_<id>/                    # images, labels and timestamps
```

`project.json` contains UUID-keyed `mcaps`, `labels`, `label_lists`, and `datasets` dictionaries.
Label lists store names and shared label UUIDs. Recording
entries include hashes, source filenames, label UUIDs and ETL references. The
`tasks` dictionary is retained for compatibility with older manifests. Existing
remote metadata is preserved when loading old projects; the local editor does
not connect to it or import downloaded remote annotations.

Registry edits use a process lock, and JSON files and completed projects/exports
are published atomically. Recording hashes protect against mismatched labels.

## Translation and synchronization scripts

Processed MCAPs include project-local workflow definitions and runnable scripts.
Use the MCAP UUID from `project.json` to replay its workflow on a source recording
with matching topics and Protobuf schemas:

```bash
python /path/to/project-name/scripts/translate.py \
  --mcap MCAP_UUID --source /path/to/source.mcap --output /path/to/translated.mcap
python /path/to/project-name/scripts/synchronize.py \
  --mcap MCAP_UUID --source /path/to/source.mcap --output /path/to/synced.mcap
```

`translate.py` skips synchronization; `synchronize.py` also applies the saved
sync settings. Their runtime is copied into the project, so these scripts work
outside this repository once `mcap` and `protobuf` are installed. Edit
`translations/<mcap-uuid>.json` and `synchronization/<mcap-uuid>.json` to change
replay behavior. The source and existing processed recordings are preserved.
Import replay outputs as new recording UUIDs. Raw imports have no configured ETL
workflow until processed in the pipeline. Ambiguous topic/schema matches omit
reusable workflows while still allowing channel-based processed exports.

## MCAP camera decoding

The adapter unwraps JSON/embedded Protobuf messages and decodes camera images and
video into a local PNG frame cache while preserving timestamps.
Supported inputs include Mono8/16, signed/float mono, RGB/BGR(A) at 8/16 bits,
Bayer and YUV422 layouts, PNG/JPEG/WebP/GIF, and H.264/H.265/VP9/AV1 packets.
Row padding and endianness are respected. Training images use RGB8 PNG; original
precision stays in the MCAP. ROS1/CDR/FlatBuffers and vendor packed pixel formats
are not decoded. Corrupt, mixed and unsupported topics have no labeling tools.
Custom translations must retain the camera fields needed for decoding. `av`
provides video decoding; `opencv-python` handles Bayer/YUV conversion. See
[ETL details](../misc/mcap-data-pipeline/ETL_README.md) and
[synthetic fixtures](../misc/SYNTHETIC_MCAP_README.md).

## Verification

```bash
python -m pip install -r requirements-dev.txt
python -m pytest -q
```

Tests cover the local annotation tools, recording isolation, named project export,
appending to existing projects, dataset exports and portable ETL replay.
