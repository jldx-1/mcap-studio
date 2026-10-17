import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { test } from "node:test";
import ts from "../frontend/node_modules/typescript/lib/typescript.js";
const { outputText } = ts.transpileModule(readFileSync(new URL("../frontend/src/utils/reviewReferences.ts", import.meta.url), "utf8"), {
  compilerOptions: { module: ts.ModuleKind.ESNext, target: ts.ScriptTarget.ES2022 },
});
const { elapsedSeconds, formatReviewTime, frameAtTime, parseReviewTime } = await import(`data:text/javascript;base64,${Buffer.from(outputText).toString("base64")}`);
const frames = [0n, 8_333_333n, 5_000_000_000n].map(offset => ({time_ns: String(1_800_000_000_000_000_001n + offset)}));
await test("timestamps retain nanosecond precision and find the closest recorded frame", () => {
  assert.equal(elapsedSeconds(frames, 1), 0.008333333);
  assert.equal(frameAtTime(frames, 0), 0);
  assert.equal(frameAtTime(frames, 0.008), 1);
  assert.equal(frameAtTime(frames, 4.9), 2);
  assert.equal(frameAtTime(frames, 5), 2);
  for (const value of [-1, 5.01, NaN, Infinity]) assert.equal(frameAtTime(frames, value), null);
  assert.equal(frameAtTime([], 0), null);
});
await test("timestamp input supports seconds, minutes and hours and rejects malformed references", () => {
  assert.equal(parseReviewTime("12.5"), 12.5);
  assert.equal(parseReviewTime("1:02.500"), 62.5);
  assert.equal(parseReviewTime("1:02:03.004"), 3723.004);
  for (const value of ["", "-1", "1:60", "1:2", "NaN", "1:02junk"]) assert.equal(parseReviewTime(value), null);
  assert.equal(formatReviewTime(59.9999), "1:00.000");
  assert.equal(formatReviewTime(3723.004), "62:03.004");
});
