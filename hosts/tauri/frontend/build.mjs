import { copyFile, mkdir, readdir, writeFile } from "node:fs/promises";
for (const file of ["index.html", "agent.html", "agent-board.html", "agent-article.html", "style.css"]) {
  await copyFile(new URL(`public/${file}`, import.meta.url), new URL(`dist/${file}`, import.meta.url));
}
// Package imports are type-checked against @leselang/bridge. The reference
// WebView has no bundler/import map: ship that same public ESM entry locally.
const sdk = new URL("dist/sdk/", import.meta.url);
const installedEntry = new URL(import.meta.resolve("@leselang/bridge"));
// The build cleans generated output before compilation. Preserve the browser
// SDK's module tree; the opt-in Node carrier cannot run in a WebView.
async function copyModules(from, to, root = false) {
  await mkdir(to, { recursive: true });
  for (const file of await readdir(from, { withFileTypes: true })) {
    if (file.isDirectory()) {
      if (root && file.name === "node") continue;
      await copyModules(new URL(`${file.name}/`, from), new URL(`${file.name}/`, to));
    } else if (file.name.endsWith(".js") || file.name.endsWith(".d.ts")) {
      await copyFile(new URL(file.name, from), new URL(file.name, to));
    }
  }
}
await copyModules(new URL(".", installedEntry), sdk, true);
await writeFile(new URL("dist/bridge.js", import.meta.url), 'export * from "./sdk/index.js";\n');
await writeFile(new URL("dist/dom-bridge.js", import.meta.url), 'export * from "./sdk/dom/index.js";\n');
