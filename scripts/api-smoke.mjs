import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import { basename } from "node:path";

// Run in a fresh, isolated workspace. Accounts and projects belong to this smoke test.
const base =
  (process.env.MCAP_API_URL || "http://127.0.0.1:8000").replace(/\/$/, "") +
  "/api";
const source = process.argv[2];
if (!source) {
  console.error(
    "Usage: node scripts/api-smoke.mjs /path/to/recording.mcap\nStart the backend with a temporary MCAP_WORKSPACE to isolate smoke-test files.",
  );
  process.exit(1);
}

let cookie = "";
const rawFetch = globalThis.fetch;
async function fetch(url, options = {}) {
  const response = await rawFetch(url,{ ...options,headers:{ ...options.headers,"X-Requested-With":"mcap-workbench",...(cookie ? {Cookie:cookie} : {}) } });
  const session = response.headers.get("set-cookie");if(session) cookie = session.split(";")[0];
  return response;
}
async function request(path, body, method = "POST") {
  const response = await fetch(
    base + path,
    body === undefined
      ? {}
      : {
          method,
          headers:
            body instanceof FormData
              ? {}
              : { "Content-Type": "application/json" },
          body: body instanceof FormData ? body : JSON.stringify(body),
        },
  );
  const text = await response.text();
  assert.ok(
    response.ok,
    `${method} ${path}: ${response.status} ${text.slice(0, 600)}`,
  );
  return JSON.parse(text);
}

const health = await request("/health");
assert.equal(health.status, "ok");
const session = await request("/auth/session");
assert.ok(session.setup_required,"Use a fresh temporary MCAP_WORKSPACE for this test.");
await request("/auth/setup",{ username:"smoke-admin",name:"Smoke administrator",password:"smoke-test-password-123" });
const adminCookie = cookie;
const groups = await request("/accounts/groups");
const reviewer = await request("/accounts/users",{ username:"smoke-reviewer",name:"Smoke reviewer",password:"smoke-test-password-123",groups:[groups.find((g) => g.name === "Reviewers").id],permissions:[] });
await request("/auth/login",{ username:"smoke-reviewer",password:"smoke-test-password-123" });
const reviewerCookie = cookie;cookie = adminCookie;
const project = await request("/projects", { name: `Smoke ${Date.now()}` });
const p = `/projects/${project.uuid}`;
await request(`${p}/members`,{ members:[{user_id:reviewer.id}] },"PUT");
const upload = new FormData();
upload.append("file", new Blob([await readFile(source)]), basename(source));
// This suite exercises image labeling; preset translations have dedicated tests.
let recording = await request(`${p}/recordings?auto_translate=false`, upload);
assert.ok(recording.topics.length > 0, "Imported MCAP must have topics.");
assert.match(recording.sha256, /^[a-f0-9]{64}$/);
const r = `${p}/recordings/${recording.uuid}`;
const image = recording.topics.find(
  (topic) => topic.kind === "image" && topic.frame_count > 0,
);
const record = recording.topics.find(
  (topic) => topic.kind === "record" && topic.frame_count > 0,
);
assert.ok(
  image,
  "Use the synthetic fixture or a recording with a supported image channel.",
);
const channel = String(image.channel_id);
const frame = recording.index.topics[channel].frames[0];
const preview = await fetch(`${base}${r}/frames/${channel}/0`);
assert.ok(preview.ok);
assert.match(preview.headers.get("content-type"), /^image\//);
assert.ok((await preview.arrayBuffer()).byteLength > 50);
if (record) assert.ok(await request(`${r}/records/${record.channel_id}/0`));

await request(`${p}/labels`, { name: "robot" });
recording = await request(r);
const state = recording.annotations;
assert.ok(
  state.classes.includes("robot"),
  "Shared class must appear in recording annotations.",
);
const classId = state.classes.indexOf("robot");
state.techniques[channel] = "yolo";
state.annotations[channel] = {
  0: {
    reviewed: true,
    boxes: [
      {
        x: 2,
        y: 2,
        width: Math.min(20, frame.width - 2),
        height: Math.min(20, frame.height - 2),
        class_id: classId,
        track_id: 1,
        origin: "manual",
      },
    ],
    labels: [],
  },
  1: { reviewed: true, boxes: [], labels: [] },
};
await request(`${r}/annotations`, state, "PUT");
const saved = await request(r);
assert.equal(
  saved.annotations.annotations[channel]["0"].boxes[0].class_id,
  classId,
);
const stale = await fetch(`${base}${r}/annotations`, {
  method: "PUT",
  headers: { "Content-Type": "application/json" },
  body: JSON.stringify({ ...state, sha256: "0".repeat(64) }),
});
assert.ok(
  stale.status >= 400 && stale.status < 500,
  "Stale recording hashes must be rejected.",
);
const negativeIndex = await fetch(`${base}${r}/frames/${channel}/999999`);
assert.ok(negativeIndex.status >= 400 && negativeIndex.status < 500);

let task = await request(`${p}/tasks`,{ title:"Smoke annotation task",recording_id:recording.uuid,scopes:[{channel_id:channel,start_frame:0,end_frame:image.frame_count-1}] });
task = await request(`${p}/tasks/${task.id}/actions`,{action:"submit",expected_version:task.version});
cookie = reviewerCookie;
task = await request(`${p}/tasks/${task.id}/actions`,{action:"claim",expected_version:task.version});
task = await request(`${p}/tasks/${task.id}/actions`,{action:"approve",expected_version:task.version});
assert.equal(task.status,"approved");cookie = adminCookie;
const published = await request(`${p}/dataset-versions`,{name:"Reviewed examples",modalities:["rgb"],sources:[{task_id:task.id,revision:1}]});
const dataset=published.dataset;assert.equal(published.version.sample_count,2);
assert.equal(published.version.source_revisions[0].revision,1);
for (const format of ["coco", "yolo", "jsonl"]) {
  const result = await request(`${p}/exports`, {
    recording_uuid: recording.uuid,
    channel_id: channel,
    format,
  });
  assert.ok(
    result.uuid && result.path && result.files.length > 0,
    `${format} export must contain files.`,
  );
}

const selected = [channel, ...(record ? [String(record.channel_id)] : [])];
const processed = await request(`${p}/pipeline`, {
  recording_uuid: recording.uuid,
  selected_channels: selected,
  translations: Object.fromEntries(selected.map((id) => [id, "* -> *"])),
  sync: {
    enabled: Boolean(record),
    parent_channel: channel,
    mode: "basic",
    tolerance_ns: null,
    delete_out_of_sync: false,
  },
});
assert.notEqual(
  processed.uuid,
  recording.uuid,
  "Processing must publish a new recording.",
);
assert.equal(processed.topics.length, selected.length);
assert.ok(processed.topics.every((topic) => topic.frame_count > 0));
assert.equal(
  (await request(p)).mcaps.length,
  2,
  "Source recording must be retained.",
);
console.log(
  `Passed: import, image/record preview, labels, annotations, hash validation, approved datasets, COCO/YOLO/JSONL export, and native pipeline.\nTest project: ${project.name}`,
);
