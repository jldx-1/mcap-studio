// Test browser upload error handling without a running server or large fixtures.
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { test } from "node:test";
import ts from "../frontend/node_modules/typescript/lib/typescript.js";

function moduleUrl(path, imports = {}) {
  let { outputText } = ts.transpileModule(readFileSync(new URL(path, import.meta.url), "utf8"), {
    compilerOptions: { module: ts.ModuleKind.ESNext, target: ts.ScriptTarget.ES2022 },
  });
  for (const [from, to] of Object.entries(imports)) outputText = outputText.replaceAll(`from "${from}"`, `from "${to}"`);
  return `data:text/javascript;base64,${Buffer.from(outputText).toString("base64")}`;
}
const http = moduleUrl("../frontend/src/services/http.ts");
const { api } = await import(http);
const { workspaceApi } = await import(moduleUrl("../frontend/src/services/workspace.ts", {
  "./http": http,
  "../utils/recordings": moduleUrl("../frontend/src/utils/recordings.ts"),
}));
const originalFetch = globalThis.fetch;

await test("oversized files report the configured limit before any upload", async () => {
  const requests = [];
  globalThis.fetch = async url => {
    requests.push(url);
    return Response.json({ max_mcap_bytes: 64 * 1024 ** 3 });
  };
  await assert.rejects(workspaceApi.importRecording("project", { name: "large.mcap", size: 65 * 1024 ** 3 }), /File is too large \(65 GiB\).*maximum MCAP upload size is 64 GiB/);
  assert.deepEqual(requests, ["/api/projects/project/upload-limits"]);
});

for (const limit of [4, null]) {
  await test(`files at the limit or with no limit can upload (${limit})`, async () => {
    const requests = [];
    globalThis.fetch = async (url, options) => {
      requests.push(url);
      if (url.endsWith("upload-limits")) return Response.json({ max_mcap_bytes: limit });
      assert.equal(options.method, "POST");
      assert.ok(options.body instanceof FormData);
      return Response.json({ uuid: "uploaded" });
    };
    assert.equal((await workspaceApi.importRecording("project", new File(["1234"], "valid.mcap"))).uuid, "uploaded");
    assert.equal(requests.length, 2);
  });
}

for (const body of ["", "<html><body>413 Request Entity Too Large</body></html>", "Payload Too Large"]) {
  await test(`proxy HTTP 413 shows a clear message without raw markup (${body.length} chars)`, async () => {
    globalThis.fetch = async () => new Response(body, { status: 413 });
    await assert.rejects(api("/upload"), error => {
      assert.match(error.message, /File is too large/);
      assert.match(error.message, /Choose a smaller file/);
      assert.doesNotMatch(error.message, /<html>/);
      return true;
    });
  });
}

await test("JSON HTTP 413 retains the backend's allowed size", async () => {
  globalThis.fetch = async () => Response.json({ error: "The file is too large. The maximum size is 64 GiB." }, { status: 413 });
  await assert.rejects(api("/upload"), /file is too large.*64 GiB/);
});

await test("other server errors retain their original message", async () => {
  globalThis.fetch = async () => Response.json({ error: "Choose an MCAP file" }, { status: 400 });
  await assert.rejects(api("/upload"), /^Error: Choose an MCAP file$/);
});

globalThis.fetch = originalFetch;
