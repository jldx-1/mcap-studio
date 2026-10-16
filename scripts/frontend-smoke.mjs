import assert from "node:assert/strict";
import { basename, resolve } from "node:path";

// Run against an isolated backend workspace. Each run creates its own projects.
// Playwright is optional; MCAP_PLAYWRIGHT_MODULE can point to an existing installation.
const { chromium } = await import(
  process.env.MCAP_PLAYWRIGHT_MODULE || "playwright"
);
const url = process.env.MCAP_UI_URL || "http://127.0.0.1:8000";
const fixtures = process.argv.slice(2).map((path) => resolve(path));
assert.equal(
  fixtures.length,
  2,
  "Usage: node scripts/frontend-smoke.mjs recording-a.mcap recording-b.mcap (two distinct MCAPs with /camera and /motor topics)",
);
const firstName = basename(fixtures[0]);
const secondName = basename(fixtures[1]);
const escapeRegex = (value) => value.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");
const browser = await chromium.launch({
  headless: true,
  args: ["--no-sandbox"],
});
try {
  const page = await browser.newPage({
    viewport: { width: 1440, height: 1050 },
    extraHTTPHeaders: { "X-Requested-With":"mcap-workbench" },
  });
  const session = await (await page.request.get(`${url}/api/auth/session`)).json();
  const signedIn = await page.request.post(`${url}/api/auth/${session.setup_required ? "setup" : "login"}`,{ data:{ username:"smoke-admin",name:"Smoke administrator",password:"smoke-test-password-123" } });
  assert.ok(signedIn.ok(),await signedIn.text());
  const errors = [];
  page.on("pageerror", (error) => errors.push(error.message));
  const get = async (path) => {
    const response = await page.request.get(`${url}/api${path}`);
    assert.ok(response.ok(), await response.text());
    return response.json();
  };
  const seed = await page.request.post(`${url}/api/projects`, {
    data: { name: `Existing ${Date.now()}` },
  });
  assert.ok(seed.ok());
  const existing = await seed.json();
  await page.addInitScript(
    (id) => localStorage.setItem("mcap-project", id),
    existing.uuid,
  );
  await page.goto(url);
  await page
    .getByRole("heading", { name: "Load a project to begin" })
    .waitFor();
  await page
    .getByRole("button", { name: "Browse server", exact: true })
    .click();
  let dialog = page.getByRole("dialog");
  await dialog
    .getByRole("button", { name: "Workspace", exact: true })
    .waitFor();
  await dialog.getByLabel("Folder path", { exact: true }).fill(existing.path);
  await dialog.getByRole("button", { name: "Browse", exact: true }).click();
  await dialog.getByRole("button", { name: "Load this project" }).click();
  await page
    .getByRole("heading", { name: existing.name, exact: true, level: 1 })
    .waitFor();
  assert.equal(
    await page.getByRole("heading", { name: "Included MCAPs" }).count(),
    1,
  );
  await page
    .getByRole("navigation", { name: "Main navigation" })
    .getByRole("button", { name: "Projects", exact: true })
    .click();
  await page.getByRole("button", { name: "New project", exact: true }).click();
  const projectName = `Frontend flow ${Date.now()}`;
  dialog = page.getByRole("dialog");
  await dialog.getByLabel("Project name").fill(projectName);
  await dialog
    .getByRole("button", { name: "Create project", exact: true })
    .click();
  await page
    .getByRole("heading", { name: projectName, exact: true, level: 1 })
    .waitFor();
  await page
    .getByLabel("Load MCAP files", { exact: true })
    .setInputFiles(fixtures);
  await page
    .getByRole("button", {
      name: new RegExp(`${escapeRegex(secondName)}.*Open topic table`),
    })
    .waitFor();
  assert.equal(
    await page.getByRole("heading", { name: "Data labeling table" }).count(),
    0,
  );
  const projects = await get("/projects");
  const project = projects.find((item) => item.name === projectName);
  const p = `/projects/${project.uuid}`;
  const detail = await get(p);
  assert.equal(detail.mcaps.length, 2);
  const first = detail.mcaps.find(
    (item) => basename(item.source_file) === firstName,
  );
  const second = detail.mcaps.find(
    (item) => basename(item.source_file) === secondName,
  );
  const open = async (name) => {
    await page
      .getByRole("button", {
        name: new RegExp(`${escapeRegex(name)}.*Open topic table`),
      })
      .click();
    await page.getByRole("heading", { name: "Data labeling table" }).waitFor();
  };
  const back = () =>
    page.getByRole("button", { name: "Back To Table", exact: true }).click();
  const editCamera = () =>
    page.getByRole("button", { name: "Edit /camera", exact: true }).click();
  const review = () =>
    page.getByRole("checkbox", { name: /Frame complete/ }).check();
  await open(firstName);
  await page
    .getByLabel("Labeling format for /camera", { exact: true })
    .selectOption("yolo");
  await page.getByRole("button", { name: "Label lists", exact: true }).click();
  dialog = page.getByRole("dialog");
  await dialog.getByLabel("New label name").fill("robot");
  await dialog.getByRole("button", { name: "Add label", exact: true }).click();
  await dialog.locator(".label-chip").filter({ hasText: "robot" }).waitFor();
  await dialog
    .getByLabel("New label list", { exact: true })
    .fill("Camera objects");
  await dialog.getByRole("checkbox", { name: "robot", exact: true }).check();
  await dialog
    .getByRole("button", { name: "Create label list", exact: true })
    .click();
  await dialog
    .locator(".list-row")
    .filter({ hasText: "Camera objects" })
    .waitFor();
  await dialog.getByRole("button", { name: "Close dialog" }).click();
  const currentProject = await get(p);
  await page
    .getByLabel("Label list for /camera", { exact: true })
    .selectOption(currentProject.label_lists[0].uuid);
  await page
    .getByRole("button", { name: "Edit /camera", exact: true })
    .waitFor({ state: "visible" });
  await editCamera();
  const canvas = page.locator('svg[aria-label^="Annotation canvas"]');
  await canvas.waitFor();
  const points = await canvas.evaluate((svg) => {
    const matrix = svg.getScreenCTM();
    return [
      [10, 10],
      [40, 40],
    ].map(([x, y]) => {
      const point = svg.createSVGPoint();
      point.x = x;
      point.y = y;
      const screen = point.matrixTransform(matrix);
      return { x: screen.x, y: screen.y };
    });
  });
  await page.mouse.move(points[0].x, points[0].y);
  await page.mouse.down();
  await page.mouse.move(points[1].x, points[1].y, { steps: 5 });
  await page.mouse.up();
  assert.equal(await page.locator(".box-row").count(), 1);
  await review();
  await page.getByRole("button", { name: "Label lists…", exact: true }).click();
  dialog = page.getByRole("dialog");
  await dialog.getByLabel("New label name").fill("obstacle");
  await dialog.getByRole("button", { name: "Add label", exact: true }).click();
  await dialog.locator(".label-chip").filter({ hasText: "obstacle" }).waitFor();
  await dialog.getByRole("button", { name: "Close dialog" }).click();
  assert.equal(await page.locator(".box-row").count(), 1);
  assert.equal(
    await page.getByRole("checkbox", { name: /Frame complete/ }).isChecked(),
    true,
  );
  await page
    .getByRole("navigation", { name: "Main navigation" })
    .getByRole("button", { name: "Projects", exact: true })
    .click();
  await page
    .getByRole("dialog")
    .getByRole("button", { name: "Cancel", exact: true })
    .click();
  await back();
  dialog = page.getByRole("dialog", {
    name: "Save annotations before leaving?",
  });
  await dialog.getByRole("button", { name: "Cancel", exact: true }).click();
  assert.equal(await page.locator(".box-row").count(), 1);
  await back();
  await page
    .getByRole("dialog")
    .getByRole("button", { name: "Save annotations", exact: true })
    .click();
  await page.getByRole("heading", { name: "Data labeling table" }).waitFor();
  let recording = await get(`${p}/recordings/${first.uuid}`);
  const camera = String(
    recording.topics.find((topic) => topic.topic === "/camera").channel_id,
  );
  const motor = String(
    recording.topics.find((topic) => topic.topic === "/motor").channel_id,
  );
  assert.equal(recording.annotations.annotations[camera]["0"].reviewed, true);
  const savedWidth =
    recording.annotations.annotations[camera]["0"].boxes[0].width;
  assert.equal(
    recording.annotations.topic_label_lists[camera],
    currentProject.label_lists[0].uuid,
  );
  await editCamera();
  await page.locator(".box-row").first().click();
  await page.getByLabel("Box width", { exact: true }).fill("35");
  await back();
  await page
    .getByRole("dialog")
    .getByRole("button", { name: "Discard changes" })
    .click();
  await page.getByRole("heading", { name: "Data labeling table" }).waitFor();
  recording = await get(`${p}/recordings/${first.uuid}`);
  assert.equal(
    recording.annotations.annotations[camera]["0"].boxes[0].width,
    savedWidth,
  );
  await editCamera();
  await page.locator(".box-row").first().click();
  await page.getByLabel("Box width", { exact: true }).fill("36");
  const annotationRoute = `**/api${p}/recordings/${first.uuid}/annotations`;
  await page.route(annotationRoute, (route) =>
    route.fulfill({
      status: 400,
      contentType: "application/json",
      body: JSON.stringify({ error: "Test save failure" }),
    }),
  );
  await back();
  dialog = page.getByRole("dialog");
  await dialog
    .getByRole("button", { name: "Save annotations", exact: true })
    .click();
  await dialog
    .getByRole("alert")
    .filter({ hasText: "Test save failure" })
    .waitFor();
  assert.equal(
    await page
      .getByRole("button", { name: "Back To Table", exact: true })
      .count(),
    1,
  );
  await page.unroute(annotationRoute);
  await dialog
    .getByRole("button", { name: "Save annotations", exact: true })
    .click();
  await page.getByRole("heading", { name: "Data labeling table" }).waitFor();
  assert.equal(
    (await get(`${p}/recordings/${first.uuid}`)).annotations.annotations[
      camera
    ]["0"].boxes[0].width,
    36,
  );
  await page.getByRole("button", { name: "Edit /motor", exact: true }).click();
  await page.getByLabel("Record annotation fields").fill("{invalid");
  await back();
  dialog = page.getByRole("dialog");
  await dialog
    .getByRole("button", { name: "Save annotations", exact: true })
    .click();
  await dialog
    .getByRole("alert")
    .filter({ hasText: "must be valid JSON" })
    .waitFor();
  await dialog.getByRole("button", { name: "Cancel", exact: true }).click();
  await page.getByLabel("Record annotation fields").fill('{"accepted":true}');
  await review();
  await back();
  await page
    .getByRole("dialog")
    .getByRole("button", { name: "Save annotations", exact: true })
    .click();
  await page.getByRole("heading", { name: "Data labeling table" }).waitFor();
  assert.deepEqual(
    (await get(`${p}/recordings/${first.uuid}`)).annotations.annotations[motor][
      "0"
    ].fields,
    { accepted: true },
  );
  await page.screenshot({ path: "/tmp/mcap-flow-topics.png", fullPage: true });
  await page
    .getByRole("button", { name: "Export annotations", exact: true })
    .click();
  dialog = page.getByRole("dialog");
  await dialog.getByLabel("Export topic", { exact: true }).selectOption(motor);
  await dialog
    .getByLabel("Export format", { exact: true })
    .selectOption("jsonl");
  await dialog
    .getByRole("button", { name: "Export JSONL", exact: true })
    .click();
  await dialog
    .getByRole("link", { name: "Download ZIP", exact: true })
    .waitFor();
  await dialog.getByRole("button", { name: "Close", exact: true }).click();
  await page
    .getByRole("button", { name: "Back to project", exact: true })
    .click();
  await open(secondName);
  await editCamera();
  await review();
  await back();
  await page
    .getByRole("dialog")
    .getByRole("button", { name: "Save annotations", exact: true })
    .click();
  await page.getByRole("heading", { name: "Data labeling table" }).waitFor();
  await page.getByRole("button", { name: "Edit /motor", exact: true }).click();
  await page.getByLabel("Record annotation fields").fill('{"draft":true}');
  await back();
  await page
    .getByRole("dialog")
    .getByRole("button", { name: "Save annotations", exact: true })
    .click();
  await page.getByRole("heading", { name: "Data labeling table" }).waitFor();
  await page
    .getByRole("button", { name: "Back to project", exact: true })
    .click();
  await page.screenshot({ path: "/tmp/mcap-flow-project.png", fullPage: true });
  await page
    .getByRole("button", { name: "Create dataset", exact: true })
    .click();
  await page
    .getByRole("dialog")
    .getByRole("button", { name: "Create a new dataset" })
    .click();
  await page
    .getByRole("heading", { name: "Create a new dataset", exact: true })
    .waitFor();
  await page
    .getByLabel("Dataset name", { exact: true })
    .fill("Cross recording dataset");
  await page
    .getByRole("checkbox", { name: "Select all visible annotated topics" })
    .check();
  assert.equal(
    await page
      .getByRole("checkbox", {
        name: `Include ${secondName} /motor`,
        exact: true,
      })
      .isDisabled(),
    true,
  );
  assert.equal(
    await page
      .getByRole("checkbox", {
        name: `Include ${firstName} /camera`,
        exact: true,
      })
      .isChecked(),
    true,
  );
  await page.screenshot({ path: "/tmp/mcap-flow-dataset.png", fullPage: true });
  const entriesRoute = `**/api${p}/datasets/*/entries`;
  let failAddition = true;
  await page.route(entriesRoute, (route) => {
    const body = route.request().postDataJSON();
    if (failAddition && body.recording_uuid === second.uuid) {
      failAddition = false;
      return route.fulfill({
        status: 400,
        contentType: "application/json",
        body: JSON.stringify({ error: "Test dataset addition failure" }),
      });
    }
    return route.continue();
  });
  await page
    .getByRole("button", { name: "Create dataset with selected data" })
    .click();
  await page
    .getByRole("alert")
    .filter({ hasText: "Test dataset addition failure" })
    .waitFor();
  assert.equal((await get(p)).datasets.length, 1);
  await page.unroute(entriesRoute);
  await page
    .getByRole("button", { name: "Add selected data", exact: true })
    .click();
  await page
    .getByRole("heading", { name: "Cross recording dataset", exact: true })
    .waitFor();
  assert.equal(await page.locator(".dataset-workspace tbody tr").count(), 3);
  const after = await get(p);
  assert.equal(after.datasets.length, 1);
  const dataset = after.datasets.find(
    (item) => item.name === "Cross recording dataset",
  );
  let membership = await get(`${p}/datasets/${dataset.uuid}`);
  assert.equal(Object.keys(membership.entries).length, 3);
  assert.equal(
    new Set(Object.values(membership.entries).map((entry) => entry.mcap_uuid))
      .size,
    2,
  );
  await page
    .getByRole("button", { name: "Add annotated data", exact: true })
    .click();
  await page
    .getByRole("checkbox", { name: "Select all visible annotated topics" })
    .check();
  await page
    .getByRole("button", { name: "Add selected data", exact: true })
    .click();
  await page
    .getByRole("heading", { name: "Cross recording dataset", exact: true })
    .waitFor();
  membership = await get(`${p}/datasets/${dataset.uuid}`);
  assert.equal(Object.keys(membership.entries).length, 3);
  await page
    .getByRole("button", { name: "Back to project", exact: true })
    .click();
  await page
    .getByRole("button", { name: "Create dataset", exact: true })
    .click();
  await page
    .getByRole("dialog")
    .getByRole("button", { name: "Load an existing dataset" })
    .click();
  await page
    .getByRole("button", { name: /Cross recording dataset.*3 entries/ })
    .click();
  await page
    .getByRole("heading", { name: "Cross recording dataset", exact: true })
    .waitFor();
  await page.setViewportSize({ width: 390, height: 844 });
  await page.waitForTimeout(300);
  await page.screenshot({ path: "/tmp/mcap-flow-mobile.png", fullPage: true });
  assert.equal(
    await page.evaluate(
      () => document.documentElement.scrollWidth > window.innerWidth,
    ),
    false,
  );
  assert.deepEqual(errors, []);
  console.log(
    "Passed: explicit project loading, folder browsing, multiple MCAP import, topic formats and label lists, image/record labeling, save/cancel/discard, failed-save recovery, multi-recording dataset selection, review gating, negatives, idempotent additions, dataset loading, and mobile layout.",
  );
} finally {
  await browser.close();
}
