import { copyFile, mkdir, readdir, rm, writeFile } from "node:fs/promises";
for (const file of ["index.html", "agent.html", "style.css"]) {
  await copyFile(new URL(file, import.meta.url), new URL(`dist/${file}`, import.meta.url));
}
// Package imports are type-checked against @leselang/bridge. The reference
// WebView has no bundler/import map: ship that same public ESM entry locally.
const sdk = new URL("dist/sdk/", import.meta.url);
await mkdir(sdk, { recursive: true });
const installedEntry = new URL(import.meta.resolve("@leselang/bridge"));
for (const file of await readdir(new URL(".", installedEntry))) {
  if (file.endsWith(".js") || file.endsWith(".d.ts")) {
    await copyFile(new URL(file, installedEntry), new URL(file, sdk));
  }
}
await writeFile(new URL("dist/bridge.js", import.meta.url), 'export * from "./sdk/index.js";\n');
// Retire the old generated form-only program from reused build directories.
for (const file of ["workflow-source.js", "workflow-source.d.ts"]) {
  await rm(new URL(`dist/${file}`, import.meta.url), { force: true });
}
