import { rm } from "node:fs/promises";

// Remove only this package's generated output before compiling moved modules.
await rm(new URL("dist/", import.meta.url), { recursive: true, force: true });
