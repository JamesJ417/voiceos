import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
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

test("server-renders the Touch system interface", async () => {
  const response = await render();
  assert.equal(response.status, 200);
  const html = await response.text();
  assert.match(html, /<title>Touch<\/title>/i);
  assert.match(html, /manifest.webmanifest/);
  assert.match(html, /Touch · VIC voice/);
  assert.match(html, /Talk with VIC/);
  assert.match(html, /Projects/);
  assert.match(html, /＋ Image/);
  assert.match(html, /VIC working/);
  assert.match(html, /Needs you/);
  assert.match(html, /Ready for review/);
  assert.match(html, /Model providers/);
  assert.match(html, /Skill proposals/);
  assert.match(html, /never enables a generated skill silently/i);
  assert.match(html, /VIC desktop presence/);
  assert.doesNotMatch(html, /codex-preview|react-loading-skeleton/i);
  const source = await readFile(new URL("../app/page.tsx", import.meta.url), "utf8");
  assert.match(source, /Focus with VIC/);
  assert.match(source, /Only this now/);
  assert.match(source, /I got interrupted/);
  assert.match(source, /Restart for 5 minutes/);
  assert.match(source, /Park without switching/);
  assert.match(source, /Idea Parking Lot/);
  assert.match(source, /Switch here safely/);
  assert.match(source, /When should this rise/);
  assert.match(source, /VoiceOS components/);
  assert.match(source, /VIC Console integration registry/);
});
