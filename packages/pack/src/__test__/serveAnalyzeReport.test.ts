import { mkdir, mkdtemp, rm, symlink, writeFile } from "node:fs/promises";
import os from "node:os";
import path from "node:path";
import { expect, it } from "vitest";
import { serveAnalyzeReport } from "../commands/serveAnalyzeReport";

it("serves report assets and data, but not files outside the report", async () => {
  const fixture = await mkdtemp(path.join(os.tmpdir(), "utoo-report-"));
  const report = path.join(fixture, "report");
  await mkdir(path.join(report, "data"), { recursive: true });
  await writeFile(path.join(report, "index.html"), "<h1>Analyzer</h1>");
  await writeFile(
    path.join(report, "data/modules.data"),
    Buffer.from([0, 1, 2]),
  );
  await writeFile(path.join(fixture, "outside.txt"), "not part of the report");
  await symlink(
    path.join(fixture, "outside.txt"),
    path.join(report, "outside.txt"),
  );
  const server = await serveAnalyzeReport(report, 0);
  try {
    const page = await fetch(server.url);
    expect(page.headers.get("content-type")).toContain("text/html");
    expect(await page.text()).toContain("Analyzer");
    const data = await fetch(`${server.url}/data/modules.data`);
    expect(Buffer.from(await data.arrayBuffer())).toEqual(
      Buffer.from([0, 1, 2]),
    );
    expect((await fetch(`${server.url}/outside.txt`)).status).toBe(404);
    expect((await fetch(`${server.url}/missing`)).status).toBe(404);
    expect((await fetch(server.url, { method: "POST" })).status).toBe(405);
    expect(await (await fetch(server.url, { method: "HEAD" })).text()).toBe("");
  } finally {
    await server.close();
    await rm(fixture, { recursive: true, force: true });
  }
});
