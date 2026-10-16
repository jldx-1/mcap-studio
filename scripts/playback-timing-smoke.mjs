// Deterministic timing/prefetch checks: no server, browser, or real-time sleeps.
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { test } from "node:test";
import ts from "../frontend/node_modules/typescript/lib/typescript.js";
const { outputText } = ts.transpileModule(readFileSync(new URL("../frontend/src/utils/framePlayback.ts", import.meta.url), "utf8"), {
  compilerOptions: { module: ts.ModuleKind.ESNext, target: ts.ScriptTarget.ES2022 },
});
const { playbackDelay, startFramePlayback } = await import(`data:text/javascript;base64,${Buffer.from(outputText).toString("base64")}`);
const flush = async () => { for (let i = 0; i < 50; i++) await Promise.resolve(); };
const framesAt = times => times.map(time => ({ time_ns: (1_800_000_000_000_000_001n + BigInt(time)).toString(), width: 640, height: 480 }));
class Clock {
  time = 0; next = 0; jobs = new Map();
  now = () => this.time;
  schedule = (callback, delay) => { const id = ++this.next; this.jobs.set(id, { at: this.time + delay, callback }); return id; };
  cancel = id => this.jobs.delete(id);
  async tick(lateness = 0) {
    const [id, job] = [...this.jobs].sort((a, b) => a[1].at - b[1].at)[0] || [];
    assert.ok(job, "expected a pending timer");
    this.jobs.delete(id); this.time = Math.max(this.time, job.at) + lateness;
    job.callback(); await flush();
  }
}
function run(clock, options) {
  return startFramePlayback({ onError: (_frame, error) => { throw error; }, onEnd: () => {}, ...clock, now: clock.now, schedule: clock.schedule, cancel: clock.cancel, ...options });
}

await test("recorded intervals retain high FPS, duplicate timestamps, long gaps and ns precision", () => {
  const frames = framesAt([0n, 8_333_333n, 8_333_333n, 5_008_333_333n]);
  assert.equal(playbackDelay(frames, 0), 8.333333);
  assert.equal(playbackDelay(frames, 1), 0);
  assert.equal(playbackDelay(frames, 2), 5000);
  assert.equal(playbackDelay(framesAt([10, 0]), 0), 120);
  assert.equal(playbackDelay([{ time_ns: "invalid" }], 0), 120);
});

for (const fps of [24, 30, 60, 120]) {
  await test(`${fps} FPS stays on the recording clock despite render time and late timers`, async () => {
    const clock = new Clock(); const seen = []; let ended = false;
    const frames = framesAt(Array.from({ length: fps * 3 + 1 }, (_, n) => BigInt(Math.round(n * 1e9 / fps))));
    run(clock, { frames, start: 0, onFrame: (number) => { seen.push([number, clock.time]); clock.time += 2; }, onEnd: () => { ended = true; } });
    await flush();
    while (!ended) await clock.tick(1);
    assert.equal(seen.length, frames.length - 1);
    assert.ok(Math.abs(seen.at(-1)[1] - 3001) < 0.000001, "render and timer overhead must not accumulate over three seconds");
  });
}

await test("variable frame intervals and long recording gaps are preserved", async () => {
  const clock = new Clock(); const seen = [];
  run(clock, { frames: framesAt([0, 10_000_000, 60_000_000, 5_060_000_000]), start: 0, onFrame: number => seen.push([number, clock.time]) });
  await flush();
  for (let n = 0; n < 3; n++) await clock.tick();
  assert.deepEqual(seen, [[1,10], [2,60], [3,5060]]);
});

await test("prefetch is bounded, concurrent, and pause frees unused images", async () => {
  const clock = new Clock(); const requested = []; const disposed = []; const visible = [];
  let inFlight = 0; let maximum = 0;
  const stop = run(clock, {
    frames: framesAt(Array.from({ length: 1000 }, (_, n) => n * 10_000_000)), start: 0,
    loadImage: async number => {
      requested.push(number); maximum = Math.max(maximum, ++inFlight);
      await Promise.resolve(); inFlight--;
      return { url: `blob:${number}`, dispose: () => disposed.push(number) };
    },
    onFrame: (number, image) => visible.push([number, image]),
  });
  await flush();
  assert.equal(maximum, 3); assert.equal(requested.length, 8);
  await clock.tick(); stop(); await flush();
  assert.equal(visible.length, 1); assert.equal(clock.jobs.size, 0);
  assert.ok(!disposed.includes(1), "pause must retain the displayed image");
  visible[0][1].dispose();
  assert.deepEqual(disposed.sort((a,b) => a-b), requested.sort((a,b) => a-b));
});

await test("large frames reduce lookahead rather than loading the whole recording", async () => {
  const clock = new Clock(); const requested = [];
  const frames = framesAt([0,1,2,3]).map(frame => ({ ...frame, width: 4096, height: 4096 }));
  const stop = run(clock, { frames, start: 0, loadImage: async number => { requested.push(number); return { url: `${number}`, dispose() {} }; }, onFrame() {} });
  await flush(); assert.deepEqual(requested, [1]); stop();
});

await test("buffering pauses the clock instead of skipping or rushing unseen frames", async () => {
  const clock = new Clock(); const seen = []; let release;
  const stop = run(clock, {
    frames: framesAt(Array.from({ length: 20 }, (_, n) => n * 10_000_000)), start: 0,
    loadImage: async number => {
      if (number === 9) await new Promise(resolve => { release = resolve; });
      return { url: `${number}`, dispose() {} };
    },
    onFrame: number => seen.push([number, clock.time]),
  });
  await flush();
  for (let n = 0; n < 9; n++) await clock.tick();
  assert.equal(seen.length, 8);
  clock.time += 100; release(); await flush();
  assert.deepEqual(seen.at(-1), [9,190]);
  await clock.tick(); assert.deepEqual(seen.at(-1), [10,200]); stop();
});

await test("pause during prefetch aborts requests and disposes late responses", async () => {
  const clock = new Clock(); let release; let disposed = 0; let signal;
  const stop = run(clock, {
    frames: framesAt([0,10_000_000]), start: 0,
    loadImage: async (_number, inputSignal) => { signal = inputSignal; await new Promise(resolve => { release = resolve; }); return { url: "late", dispose: () => { disposed++; } }; },
    onFrame: () => assert.fail("advanced after pause"),
  });
  await flush(); stop(); assert.equal(signal.aborted, true);
  release(); await flush(); assert.equal(disposed, 1); assert.equal(clock.jobs.size, 0);
});

await test("frame errors stop cleanly and resume/replay start from the selected frame", async () => {
  const clock = new Clock(); const errors = [];
  run(clock, { frames: framesAt([0,10_000_000,20_000_000]), start: 1, loadImage: async () => { throw new Error("load failed"); }, onFrame: () => assert.fail("displayed a failed image"), onError: (number,error) => errors.push([number,error.message]) });
  await flush(); await clock.tick(); assert.deepEqual(errors, [[2,"load failed"]]);
  assert.equal(clock.jobs.size, 0);
  const shown = []; let ended = false;
  run(clock, { frames: framesAt([0,10_000_000,20_000_000]), start: 1, onFrame: number => shown.push(number), onEnd: () => { ended = true; } });
  await flush(); await clock.tick(); assert.deepEqual(shown,[2]); assert.equal(ended,true);
});
