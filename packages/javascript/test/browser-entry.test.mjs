import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import test from "node:test";
import ts from "typescript";

const root = new URL("../", import.meta.url);
const dist = new URL("dist/", root);

// Check emitted modules, including transitive imports and declaration-only
// imports. A Node test importing the browser entry would otherwise hide an
// accidental dependency on Node builtins or ambient globals.
async function browserGraph(entry, declarations) {
  const visited = new Set();
  async function visit(url) {
    assert.ok(url.href.startsWith(dist.href), `module escaped the package: ${url}`);
    assert.ok(!url.href.startsWith(new URL("node/", dist).href), `browser entry reached Node carrier: ${url}`);
    if (visited.has(url.href)) return;
    visited.add(url.href);
    const source = await readFile(url, "utf8");
    const file = ts.createSourceFile(url.pathname, source, ts.ScriptTarget.Latest, true,
      declarations ? ts.ScriptKind.TS : ts.ScriptKind.JS);
    const imports = [];
    const add = (node) => {
      assert.ok(ts.isStringLiteralLike(node), `non-literal import in ${url}`);
      imports.push(node.text);
    };
    function inspect(node) {
      if ((ts.isImportDeclaration(node) || ts.isExportDeclaration(node)) && node.moduleSpecifier) add(node.moduleSpecifier);
      if (ts.isImportTypeNode(node)) {
        assert.ok(ts.isLiteralTypeNode(node.argument), `non-literal type import in ${url}`);
        add(node.argument.literal);
      }
      if (ts.isCallExpression(node) && node.expression.kind === ts.SyntaxKind.ImportKeyword) add(node.arguments[0]);
      if (ts.isIdentifier(node)) {
        assert.ok(!["Buffer", "process", "require", "__dirname", "__filename", "NodeJS"].includes(node.text),
          `Node ambient ${node.text} in browser module ${url}`);
      }
      ts.forEachChild(node, inspect);
    }
    inspect(file);
    assert.equal(file.typeReferenceDirectives.length, 0, `ambient type dependency in ${url}`);
    for (const dependency of imports) {
      assert.ok(dependency.startsWith("./") || dependency.startsWith("../"),
        `browser module ${url} depends on external ${dependency}`);
      const target = declarations ? dependency.replace(/\.js$/u, ".d.ts") : dependency;
      await visit(new URL(target, url));
    }
  }
  await visit(entry);
  return visited;
}

test("the public browser runtime and declarations never reach the opt-in Node carrier", async () => {
  const manifest = JSON.parse(await readFile(new URL("package.json", root), "utf8"));
  const browser = manifest.exports["."];
  const node = manifest.exports["./node"];
  const dom = manifest.exports["./dom"];
  const operations = manifest.exports["./operations"];
  assert.equal(typeof node.import, "string", "the Node carrier has its own public entry");
  assert.equal(typeof node.types, "string", "the Node carrier has its own public declarations");
  const runtime = await browserGraph(new URL(browser.import, root), false);
  const declarations = await browserGraph(new URL(browser.types, root), true);
  assert.ok(runtime.has(new URL("bridge.js", dist).href), "the real Bridge is checked transitively");
  assert.ok(runtime.has(new URL("agent/registered-host.js", dist).href), "the registered host is checked transitively");
  assert.ok(declarations.has(new URL("agent/registered-host.d.ts", dist).href));
  assert.ok(!runtime.has(new URL(node.import, root).href));
  assert.ok(!declarations.has(new URL(node.types, root).href));
  const domRuntime = await browserGraph(new URL(dom.import, root), false);
  const domDeclarations = await browserGraph(new URL(dom.types, root), true);
  assert.ok(!runtime.has(new URL(dom.import, root).href), "the root entry does not eagerly import DOM bindings");
  assert.ok(!domRuntime.has(new URL(node.import, root).href));
  assert.ok(!domDeclarations.has(new URL(node.types, root).href));
  await browserGraph(new URL(operations.import, root), false);
  await browserGraph(new URL(operations.types, root), true);
  await readFile(new URL(node.import, root), "utf8");
  await readFile(new URL(node.types, root), "utf8");
});
