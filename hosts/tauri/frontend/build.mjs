import { copyFile, mkdir, readdir, rm, writeFile } from "node:fs/promises";
for (const file of ["index.html", "agent.html", "style.css"]) {
  await copyFile(new URL(`public/${file}`, import.meta.url), new URL(`dist/${file}`, import.meta.url));
}
// Package imports are type-checked against @leselang/bridge. The reference
// WebView has no bundler/import map: ship that same public ESM entry locally.
const sdk = new URL("dist/sdk/", import.meta.url);
const installedEntry = new URL(import.meta.resolve("@leselang/bridge"));
// Refresh only this generated SDK copy so moved private modules cannot leave
// stale files behind. Preserve its module tree exactly as shipped by the SDK.
await rm(sdk, { recursive: true, force: true });
async function copyModules(from, to) {
  await mkdir(to, { recursive: true });
  for (const file of await readdir(from, { withFileTypes: true })) {
    if (file.isDirectory()) {
      await copyModules(new URL(`${file.name}/`, from), new URL(`${file.name}/`, to));
    } else if (file.name.endsWith(".js") || file.name.endsWith(".d.ts")) {
      await copyFile(new URL(file.name, from), new URL(file.name, to));
    }
  }
}
await copyModules(new URL(".", installedEntry), sdk);
await writeFile(new URL("dist/bridge.js", import.meta.url), 'export * from "./sdk/index.js";\n');
// Retire the old generated form-only program from reused build directories.
for (const file of ["workflow-source.js", "workflow-source.d.ts"]) {
  await rm(new URL(`dist/${file}`, import.meta.url), { force: true });
}
