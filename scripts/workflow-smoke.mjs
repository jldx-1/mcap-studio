import assert from "node:assert/strict";
import { readFile, mkdir, copyFile } from "node:fs/promises";
import { join } from "node:path";

// Run against an isolated backend workspace. See docs/STREAMLINED_WORKFLOW.md.
const { chromium } = await import(process.env.MCAP_PLAYWRIGHT_MODULE || "playwright");
const url = process.env.MCAP_UI_URL || "http://127.0.0.1:8137";
const fixtures = process.argv[2];
assert.ok(fixtures, "Usage: node scripts/workflow-smoke.mjs /path/to/workflow-fixtures");
const browser = await chromium.launch({ headless: true, executablePath: process.env.MCAP_BROWSER_EXECUTABLE || undefined, args: ["--no-sandbox"] });
const context = await browser.newContext({ viewport: { width: 1440, height: 1100 }, extraHTTPHeaders: { "X-Requested-With": "mcap-workbench" } });
const reviewer = await browser.newContext({ extraHTTPHeaders: { "X-Requested-With": "mcap-workbench" } });
const errors = [];
const page = await context.newPage();
page.on("pageerror", error => errors.push(error.message));
const call = async (client, path, body, method = "post") => {
  const response = body === undefined ? await client.request.get(`${url}/api${path}`) : await client.request[method](`${url}/api${path}`, { data: body });
  assert.ok(response.ok(), `${path}: ${response.status()} ${await response.text()}`); return response.json();
};
const until = async (fn, description, timeout = 30000) => {
  const start = Date.now(); while (Date.now() - start < timeout) { const value = await fn(); if (value) return value; await new Promise(resolve => setTimeout(resolve, 200)); }
  throw new Error(`Timed out: ${description}`);
};
let project;
try {
  const credentials = { username: "admin", name: "Administrator", password: "workflow-test-password-123" };
  const session = await call(context, "/auth/session");
  assert.ok(session.setup_required || process.env.MCAP_SMOKE_REUSE === "1", "Use a fresh isolated workspace");
  await call(context, session.setup_required ? "/auth/setup" : "/auth/login", credentials);
  const groups = await call(context, "/accounts/groups");
  const username = `reviewer${Date.now()}`;
  const bob = await call(context, "/accounts/users", { username, name: "Workflow reviewer", password: credentials.password, groups: [groups.find(g => g.name === "Reviewers").id], permissions: [] });
  await call(reviewer, "/auth/login", { username, password: credentials.password });
  project = await call(context, "/projects", { name: `Workflow smoke ${Date.now()}` });
  const base = `/projects/${project.uuid}`;
  await call(context, `${base}/members`, { members: [{ user_id: bob.id }] }, "put");
  project = await call(context, `${base}/labels`, { name: "robot" });
  project = await call(context, `${base}/label-lists`, { name: "Objects", label_ids: [project.labels[0].uuid] });
  await page.goto(url);
  await page.getByRole("button", { name: new RegExp(project.name) }).click();
  const files = await Promise.all(["session-1.mcap", "session-2.mcap", "motor.json"].map(async name => ({ name, bytes: (await readFile(join(fixtures, name))).toString("base64") })));
  await page.evaluate(rows => {
    const transfer = new DataTransfer();
    for (const row of rows) transfer.items.add(new File([Uint8Array.from(atob(row.bytes), c => c.charCodeAt(0))], row.name));
    document.dispatchEvent(new DragEvent("drop", { dataTransfer: transfer, bubbles: true, cancelable: true }));
  }, files);
  await page.getByRole("heading", { name: "Prepare session-1.mcap", exact: true }).waitFor();
  const debug = page.locator(".preparation-topic").filter({ has: page.getByText("/debug", { exact: true }) });
  await debug.locator('input[type="checkbox"]').first().uncheck();
  const camera = page.locator(".preparation-topic").filter({ has: page.getByText("/camera", { exact: true }) });
  await camera.getByLabel("Role / labeling format").selectOption("classification");
  await camera.getByLabel("Label list", { exact: true }).selectOption(project.label_lists[0].uuid);
  assert.equal(await page.getByLabel("Preparation translation for /motor").inputValue(), "0");
  await page.getByLabel("Recipe name", { exact: true }).fill("Robot training");
  await page.getByRole("button", { name: "Save recipe", exact: true }).click();
  await until(async () => (await call(context, `${base}/workflow/recipes`)).length === 1, "saved preparation recipe");
  await page.getByRole("button", { name: "Apply these settings to remaining recordings", exact: true }).click();
  const jobs = await until(async () => { const jobs = await call(context, `${base}/workflow/jobs`); assert.ok(!jobs.some(j => j.status === "failed"), JSON.stringify(jobs)); return jobs.length === 2 && jobs.every(j => j.status === "ready") ? jobs : null; }, "background preparations");
  const outputs = jobs.map(j => j.output_recording).sort();
  for (const id of outputs) {
    const recording = await call(context, `${base}/recordings/${id}`);
    assert.deepEqual(recording.topics.map(t => t.topic).sort(), ["/camera", "/motor"]);
    const motor = recording.topics.find(t => t.topic === "/motor");
    assert.ok((await call(context, `${base}/recordings/${id}/records/${motor.channel_id}/0`)).position_mm >= 1000);
  }
  await page.reload();
  await page.getByRole("button", { name: new RegExp(project.name) }).click();
  await page.getByRole("button", { name: "Continue labeling queue", exact: true }).click();
  await page.locator(".review-check input").check();
  const firstOutput = await until(async () => { for (const id of outputs) { const rec = await call(context, `${base}/recordings/${id}`); if (rec.annotations.annotations["1"]?.["0"]?.explicit_empty) return id; } }, "autosaved confirmed negative");
  await page.getByRole("button", { name: "Next incomplete frame", exact: true }).click();
  assert.equal(await page.getByLabel("Frame number", { exact: true }).inputValue(), "2");
  await page.getByRole("checkbox", { name: "robot", exact: true }).check();
  await until(async () => (await call(context, `${base}/recordings/${firstOutput}`)).annotations.annotations["1"]?.["1"]?.labels?.length === 1, "autosaved class label");
  await page.getByRole("button", { name: "Save and next topic / recording", exact: true }).click();
  await page.getByRole("checkbox", { name: "robot", exact: true }).check();
  const secondOutput = outputs.find(id => id !== firstOutput);
  await until(async () => (await call(context, `${base}/recordings/${secondOutput}`)).annotations.annotations["1"]?.["0"]?.labels?.length === 1, "next recording autosave");
  await page.getByRole("navigation", { name: "Project navigation" }).getByRole("button", { name: "Datasets", exact: true }).click();
  await page.getByRole("button", { name: "Choose recordings", exact: true }).click();
  for (const id of outputs) { const rec = await call(context, `${base}/recordings/${id}`); await page.getByRole("checkbox", { name: `Include recording ${rec.source_file}`, exact: true }).check(); }
  await page.getByLabel("Draft name", { exact: true }).fill("Training selection");
  await page.getByRole("button", { name: "Save draft", exact: true }).click();
  const draft = await until(async () => (await call(context, `${base}/workflow/drafts`))[0], "saved pending dataset draft");
  assert.equal(draft.recordings.length, 2);
  const approve = async id => {
    let task = await call(context, `${base}/tasks`, { title: `Camera ${id}`, recording_id: id, scopes: [{ channel_id: "1", start_frame: 0, end_frame: 3 }] });
    task = await call(context, `${base}/tasks/${task.id}/actions`, { action: "submit", expected_version: task.version });
    task = await call(reviewer, `${base}/tasks/${task.id}/actions`, { action: "claim", expected_version: task.version });
    return call(reviewer, `${base}/tasks/${task.id}/actions`, { action: "approve", expected_version: task.version });
  };
  await approve(firstOutput);
  await page.getByRole("button", { name: "Refresh recordings", exact: true }).click();
  await page.getByRole("heading", { name: "Create dataset from approved data", exact: true }).waitFor();
  await page.getByLabel("Dataset name", { exact: true }).fill("Robot training set");
  await page.getByRole("button", { name: "Create dataset version", exact: true }).click();
  await page.getByRole("heading", { name: "Robot training set · Version 1", exact: true }).waitFor();
  project = await call(context, base);
  const dataset = project.datasets.find(d => d.name === "Robot training set");
  const old = await call(context, `${base}/datasets/${dataset.uuid}/versions/1`);
  assert.equal(old.source_revisions.length, 1);
  await approve(secondOutput);
  await page.getByRole("button", { name: "Create new dataset version", exact: true }).click();
  const second = await call(context, `${base}/recordings/${secondOutput}`);
  await page.getByRole("checkbox", { name: `Include recording ${second.source_file}`, exact: true }).check();
  await page.getByRole("button", { name: "Refresh recordings", exact: true }).click();
  await until(async () => (await page.locator(".dataset-estimate").textContent())?.includes("1 retained · 1 added"), "preserved prior dataset membership");
  await page.getByText("Choose exact approved revisions and filter labels or modalities", { exact: true }).click();
  await page.getByLabel("MCAP search", { exact: true }).fill("not-present");
  assert.ok((await page.locator(".dataset-estimate").textContent()).includes("1 retained · 1 added"));
  await page.getByRole("button", { name: "Publish next dataset version", exact: true }).click();
  await page.getByRole("heading", { name: "Robot training set · Version 2", exact: true }).waitFor();
  const next = await call(context, `${base}/datasets/${dataset.uuid}/versions/2`);
  assert.equal(next.source_revisions.length, 2);
  assert.deepEqual(await call(context, `${base}/datasets/${dataset.uuid}/versions/1`), old);
  const built = await call(context, `${base}/datasets/${dataset.uuid}/versions/2/build`, { format: "csv" });
  assert.ok(built.sample_count > 0);
  const samples = await call(context, `${base}/datasets/${dataset.uuid}/versions/2/samples?offset=0&limit=50`);
  assert.ok(samples.samples.every(s => ["train", "val", "test"].includes(s.split)));
  await page.setViewportSize({ width: 390, height: 844 });
  assert.equal(await page.evaluate(() => document.documentElement.scrollWidth > innerWidth + 2), false, "mobile layout must not overflow");
  await page.screenshot({ path: "/tmp/mcap-workflow-mobile.png", fullPage: true });
  await page.setViewportSize({ width: 1440, height: 1100 });
  await page.screenshot({ path: "/tmp/mcap-workflow-desktop.png", fullPage: true });
  if (process.env.MCAP_TEST_WORKSPACE) {
    await mkdir(join(process.env.MCAP_TEST_WORKSPACE, "imports"), { recursive: true });
    await copyFile(join(fixtures, "session-3.mcap"), join(process.env.MCAP_TEST_WORKSPACE, "imports/session-3.mcap"));
    await page.getByRole("navigation", { name: "Project navigation" }).getByRole("button", { name: "Recordings", exact: true }).click();
    await page.getByRole("button", { name: "Load from server", exact: true }).click();
    await page.getByRole("checkbox", { name: /session-3.mcap/ }).check();
    await page.getByRole("button", { name: "Load 1 recordings", exact: true }).click();
    await page.getByRole("heading", { name: "Prepare session-3.mcap", exact: true }).waitFor();
    const recipes = await call(context, `${base}/workflow/recipes`);
    await page.getByLabel("Saved preparation recipe", { exact: true }).selectOption(recipes[0].uuid);
    await until(async () => !(await page.locator(".preparation-topic").filter({ has: page.getByText("/debug", { exact: true }) }).locator('input[type="checkbox"]').first().isChecked()), "recipe topic removal on reused recording");
  }
  assert.deepEqual(errors, []);
  console.log("Workflow browser smoke passed: mixed drop, recipe, filtered preparation, queue recovery, autosave, pending draft, preserved dataset members, splits, immutable versions, server import, mobile layout.");
} catch (error) {
  await page.screenshot({ path: "/tmp/mcap-workflow-failure.png", fullPage: true }).catch(() => {});
  console.error("Project:", project?.uuid, "Page errors:", errors);
  throw error;
} finally {
  await browser.close();
}
