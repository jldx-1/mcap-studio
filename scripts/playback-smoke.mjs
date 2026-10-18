import assert from "node:assert/strict";
import {copyFile, mkdtemp, readFile, rm} from "node:fs/promises";
import {basename, join} from "node:path";
import {tmpdir} from "node:os";

const {chromium} = await import(process.env.MCAP_PLAYWRIGHT_MODULE || "playwright");
const url = process.env.MCAP_UI_URL || "http://127.0.0.1:8000";
const fixture = process.argv[2];
assert.ok(fixture, "Usage: node scripts/playback-smoke.mjs /path/to/image-recording.mcap");
const browser = await chromium.launch({headless:true, args:["--no-sandbox"]});
const uploadDirectory = await mkdtemp(join(tmpdir(), "mcap-playback-upload-"));
try {
  const page = await browser.newPage({viewport:{width:1440,height:1050},extraHTTPHeaders:{"X-Requested-With":"mcap-workbench"}});
  const session = await (await page.request.get(`${url}/api/auth/session`)).json();
  const signedIn = await page.request.post(`${url}/api/auth/${session.setup_required ? "setup" : "login"}`,{ data:{ username:"smoke-admin",name:"Smoke administrator",password:"smoke-test-password-123" } });
  assert.ok(signedIn.ok(),await signedIn.text());
  const errors = [];
  page.on("pageerror", (error) => errors.push(error.message));
  const request = async (path, data) => {
    const response = data === undefined ? await page.request.get(`${url}/api${path}`) : await page.request.post(`${url}/api${path}`, {data});
    assert.ok(response.ok(), await response.text());
    return response.json();
  };
  let project = await request("/projects", {name:`Playback check ${Date.now()}`});
  const base = `/projects/${project.uuid}`;
  const upload = await page.request.post(`${url}/api${base}/recordings?defer_index=true&auto_translate=false`, {
    multipart:{file:{name:basename(fixture), mimeType:"application/octet-stream", buffer:await readFile(fixture)}},
  });
  assert.ok(upload.ok(), await upload.text());
  const imported = await upload.json();
  assert.equal(imported.indexing,"pending");
  const recording = await request(`${base}/recordings/${imported.uuid}`);
  const topic = recording.topics.find((item) => item.kind === "image" && item.frame_count >= 3 && (!process.env.MCAP_PLAYBACK_TOPIC || item.topic === process.env.MCAP_PLAYBACK_TOPIC));
  assert.ok(topic,"Fixture needs at least three image frames");
  const channel = String(topic.channel_id);
  project = await request(`${base}/labels`,{name:"robot"});
  project = await request(`${base}/label-lists`,{name:"Objects",label_ids:[project.labels[0].uuid]});
  const state = (await request(`${base}/recordings/${recording.uuid}`)).annotations;
  state.techniques[channel] = "yolo";
  state.topic_label_lists = {[channel]:project.label_lists[0].uuid};
  const saved = await page.request.put(`${url}/api${base}/recordings/${recording.uuid}/annotations`,{data:state});
  assert.ok(saved.ok(),await saved.text());
  let jobStatus="running";
  await page.route(`**/api${base}/workflow/jobs`,route=>route.fulfill({json:[{uuid:"browser-status-job",recording_uuid:recording.uuid,source_file:basename(fixture),status:jobStatus,stage:jobStatus==="running"?"Indexing frames":"Ready",output_recording:jobStatus==="ready"?recording.uuid:undefined,request:{}}]}));
  // Existing derived MCAPs appear as one recording, without version windows.
  await page.route(`**/api${base}`,async route=>{
    const response=await route.fetch();const value=await response.json();
    const current=value.mcaps.find(item=>item.uuid===recording.uuid);
    value.mcaps=[{...current,uuid:"source-for-browser-check",created_at:"2020-01-01T00:00:00Z"},{...current,source_recording_uuid:"source-for-browser-check"}];
    await route.fulfill({response,json:value});
  });
  await page.goto(url);
  await page.getByRole("button",{name:new RegExp(project.name)}).click();
  await page.getByRole("heading",{name:project.name,exact:true,level:1}).waitFor();
  await page.getByRole("navigation",{name:"Project navigation"}).getByRole("button",{name:"MCAP Library",exact:true}).click();
  await page.getByLabel("Sort MCAPs by import date",{exact:true}).selectOption("oldest");
  const uploadResponse = page.waitForResponse((response) => response.url().includes("/recordings?defer_index=true") && response.request().method() === "POST").catch(() => null);
  const additionalRecording = join(uploadDirectory, "additional-recording.mcap");
  await copyFile(fixture, additionalRecording);
  await page.getByLabel("Load MCAP files",{exact:true}).setInputFiles(additionalRecording);
  const uploadedResponse = await uploadResponse;assert.ok(uploadedResponse?.ok(),"Browser upload should succeed");
  assert.equal((await uploadedResponse.json()).duplicate,true,"Uploading the same MCAP bytes should reuse the recording");
  await page.getByRole("heading",{name:"MCAP Library",exact:true,level:1}).waitFor();
  assert.equal(await page.locator(".preparation-jobs,.recording-family").count(),0,"Recording library has no job panel or version windows");
  await page.getByRole("button",{name:/^Notifications/}).waitFor();
  assert.equal(await page.locator(".recordings-table tbody tr").count(),1,"Original and derived data share one library entry");
  await page.getByRole("button",{name:/^Notifications/}).click();
  const notifications=page.getByRole("region",{name:"Notifications",exact:true});
  await notifications.getByText("Processing",{exact:true}).waitFor();
  jobStatus="ready";
  await notifications.getByText("Ready to label",{exact:true}).waitFor();
  await notifications.getByRole("button",{name:"Start labeling",exact:true}).waitFor();
  await page.getByRole("button",{name:"Notifications",exact:true}).waitFor();
  await page.getByRole("button",{name:"Close notifications",exact:true}).click();
  assert.equal(await page.getByLabel("Sort MCAPs by import date",{exact:true}).inputValue(),"oldest","Import must preserve the selected sort order");
  await page.getByRole("button",{name:`Open recording ${basename(fixture)}`,exact:true}).first().click();
  await page.getByRole("button",{name:`Create Labels for ${topic.topic}`,exact:true}).click();
  const counter = page.getByLabel("Frame number",{exact:true});
  await counter.waitFor();
  const canvas = page.locator('svg[aria-label^="Annotation canvas"]');
  await canvas.waitFor();
  // Make each response slower than the frame interval and disable browser HTTP
  // caching. Playback must still display actual images rather than cancel them.
  await page.route(`**/frames/${channel}/*`,async (route) => {
    await new Promise((resolve) => setTimeout(resolve,180));
    try {await route.continue();} catch { /* A canceled prefetch after pause is expected. */ }
  });
  await page.getByRole("button",{name:"Play frames",exact:true}).click();
  await page.waitForFunction(() => Number(document.querySelector('input[aria-label="Frame number"]').value) >= 3);
  await page.getByRole("button",{name:"Pause playback",exact:true}).click();
  const paused = await counter.inputValue();
  assert.ok(Number(paused) >= 3);
  assert.match(await canvas.locator("image").getAttribute("href"),/^blob:/);
  await new Promise((resolve) => setTimeout(resolve,400));
  assert.equal(await counter.inputValue(),paused,"Pause must cancel pending advancement");
  assert.equal(await page.getByRole("checkbox",{name:/Frame complete|Skip this frame/}).count(),0);
  await page.getByLabel("Annotation type",{exact:true}).selectOption("classification");
  await page.getByRole("checkbox",{name:"robot",exact:true}).check();
  const annotationResponse = page.waitForResponse((response) => response.url().endsWith("/annotations") && response.request().method() === "PATCH");
  await page.getByRole("button",{name:"Save annotations",exact:true}).click();
  const patchResponse = await annotationResponse;
  assert.ok(patchResponse.ok(),await patchResponse.text());
  assert.ok((await patchResponse.json()).metadata);
  await page.getByText("All changes saved",{exact:true}).waitFor();
  assert.deepEqual((await request(`${base}/recordings/${recording.uuid}`)).annotations.annotations[channel][String(Number(paused)-1)].labels,[0]);
  // Native Space activation of a focused Play button must toggle exactly once.
  await page.getByRole("button",{name:"Play frames",exact:true}).press("Space");
  await page.waitForFunction((previous) => Number(document.querySelector('input[aria-label="Frame number"]').value) > previous,Number(paused));
  await page.getByRole("button",{name:"Pause playback",exact:true}).click();
  await page.getByRole("button",{name:"Last frame",exact:true}).click();
  assert.equal(Number(await counter.inputValue()),topic.frame_count);
  await page.getByRole("button",{name:"Play frames",exact:true}).click();
  await page.waitForFunction(() => Number(document.querySelector('input[aria-label="Frame number"]').value) === 1);
  await page.waitForFunction(() => Number(document.querySelector('input[aria-label="Frame number"]').value) >= 2);
  await page.getByRole("button",{name:"Pause playback",exact:true}).click();
  assert.deepEqual(errors,[]);
  assert.equal(await page.getByText("Frame preview unavailable",{exact:true}).count(),0);
  await page.screenshot({path:"/tmp/mcap-playback-check.png",fullPage:true});
  console.log("Passed: duplicate MCAP import stays in the recording library and preserves sort order, compact annotation saving, displayed playback under slow responses, pause, focused Space, and replay from the end.");
} finally {await browser.close();await rm(uploadDirectory,{recursive:true,force:true});}
