import assert from "node:assert/strict";
import {readFile} from "node:fs/promises";
import {basename} from "node:path";

const {chromium} = await import(process.env.MCAP_PLAYWRIGHT_MODULE || "playwright");
const url = process.env.MCAP_UI_URL || "http://127.0.0.1:8000";
const fixture = process.argv[2];
assert.ok(fixture, "Usage: node scripts/playback-smoke.mjs /path/to/image-recording.mcap");
const browser = await chromium.launch({headless:true, args:["--no-sandbox"]});
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
  const upload = await page.request.post(`${url}/api${base}/recordings?defer_index=true`, {
    multipart:{file:{name:basename(fixture), mimeType:"application/octet-stream", buffer:await readFile(fixture)}},
  });
  assert.ok(upload.ok(), await upload.text());
  const imported = await upload.json();
  assert.equal(imported.indexing,"pending");
  const recording = await request(`${base}/recordings/${imported.uuid}`);
  const topic = recording.topics.find((item) => item.kind === "image" && item.frame_count >= 3);
  assert.ok(topic,"Fixture needs at least three image frames");
  const channel = String(topic.channel_id);
  project = await request(`${base}/labels`,{name:"robot"});
  project = await request(`${base}/label-lists`,{name:"Objects",label_ids:[project.labels[0].uuid]});
  const state = (await request(`${base}/recordings/${recording.uuid}`)).annotations;
  state.techniques[channel] = "yolo";
  state.topic_label_lists = {[channel]:project.label_lists[0].uuid};
  const saved = await page.request.put(`${url}/api${base}/recordings/${recording.uuid}/annotations`,{data:state});
  assert.ok(saved.ok(),await saved.text());
  await page.goto(url);
  await page.getByRole("button",{name:new RegExp(project.name)}).click();
  await page.getByRole("heading",{name:project.name,exact:true,level:1}).waitFor();
  await page.getByRole("navigation",{name:"Project navigation"}).getByRole("button",{name:"MCAPs",exact:true}).click();
  await page.getByLabel("Sort MCAPs by import date",{exact:true}).selectOption("oldest");
  const uploadResponse = page.waitForResponse((response) => response.url().includes("/recordings?defer_index=true") && response.request().method() === "POST");
  await page.getByLabel("Load MCAP files",{exact:true}).setInputFiles({name:"additional-recording.mcap",mimeType:"application/octet-stream",buffer:await readFile(fixture)});
  assert.ok((await uploadResponse).ok());
  await page.getByRole("button",{name:/additional-recording\.mcap.*Open topic table/}).waitFor();
  assert.equal(await page.getByRole("heading",{name:"Project MCAPs",exact:true,level:1}).isVisible(),true,"Import must preserve the MCAPs section");
  assert.equal(await page.getByLabel("Sort MCAPs by import date",{exact:true}).inputValue(),"oldest","Import must preserve the selected sort order");
  await page.getByRole("button",{name:new RegExp("Open topic table")}).first().click();
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
  await page.getByRole("checkbox",{name:/Frame complete/}).check();
  const annotationResponse = page.waitForResponse((response) => response.url().endsWith("/annotations") && response.request().method() === "PATCH");
  await page.getByRole("button",{name:"Save annotations",exact:true}).click();
  const patchResponse = await annotationResponse;
  assert.ok(patchResponse.ok(),await patchResponse.text());
  assert.ok((await patchResponse.json()).metadata);
  await page.getByText("All changes saved",{exact:true}).waitFor();
  assert.equal((await request(`${base}/recordings/${recording.uuid}`)).annotations.annotations[channel][String(Number(paused)-1)].reviewed,true);
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
  console.log("Passed: MCAP import preserves its section/sort order, compact annotation saving, displayed playback under slow responses, pause, focused Space, and replay from the end.");
} finally {await browser.close();}
