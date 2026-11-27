// Compare observations emitted by the three standalone Consumer processes.
import assert from "node:assert/strict";
import { mkdir, mkdtemp, readFile, writeFile } from "node:fs/promises";
import { join, resolve } from "node:path";
import { consumerCommands, publicationFixture, root, runConsumer, verifiedObservations } from "./registered-gui-consumers.mjs";

const fixture = resolve(process.argv[2] ?? publicationFixture);
const { cases } = JSON.parse(await readFile(fixture, "utf8"));
const expected = new Map(cases.map(({ id, expected }) => [id, expected]));
assert.ok(expected.size > 0 && expected.size === cases.length, "fixture requires distinct case IDs");

const parent = join(root, "target/verification");
await mkdir(parent, { recursive: true });
const output = await mkdtemp(join(parent, "gui-consumers."));
console.log(`GUI Consumer observations: ${output}`);
let reference;
for (const command of consumerCommands(fixture)) {
  const [name] = command;
  const records = verifiedObservations(runConsumer(command), expected, name);
  await writeFile(join(output, name + ".json"), JSON.stringify(Object.fromEntries(records), null, 2) + "\n");
  if (reference) assert.deepEqual(records, reference, `${name}: observations differ from Rust`);
  else reference = records;
}
for (const [id, { outcome, verified, commits }] of reference) {
  console.log(`${id}: ${outcome}, verified=${verified}, edits=${commits.edits}, clicks=${commits.clicks}`);
}
console.log(`All ${reference.size} GUI cases matched across Rust, Node and C#.`);
