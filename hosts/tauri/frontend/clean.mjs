import { rm } from "node:fs/promises";

// Source modules and public assets live outside this fixed generated directory.
await rm(new URL("dist/", import.meta.url), { recursive: true, force: true });
