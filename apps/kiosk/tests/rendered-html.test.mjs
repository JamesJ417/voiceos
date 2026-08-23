import assert from "node:assert/strict";
import test from "node:test";

async function render() {
  const workerUrl = new URL("../dist/server/index.js", import.meta.url);
  workerUrl.searchParams.set("test", `${process.pid}-${Date.now()}`);
  const { default: worker } = await import(workerUrl.href);
  return worker.fetch(
    new Request("http://localhost/", { headers: { accept: "text/html" } }),
    { ASSETS: { fetch: async () => new Response("Not found", { status: 404 }) } },
    { waitUntil() {}, passThroughOnException() {} },
  );
}

test("server-renders the Carbon Command surface", async () => {
  const response = await render();
  assert.equal(response.status, 200);
  const html = await response.text();
  assert.match(html, /<title>Omarchy Voice Carbon Command<\/title>/i);
  assert.match(html, /Carbon Command/);
  assert.match(html, /Command center/);
  assert.match(html, /Model providers/);
  assert.match(html, /Skill proposals/);
  assert.match(html, /never enables a generated skill silently/i);
  assert.doesNotMatch(html, /codex-preview|react-loading-skeleton/i);
});
