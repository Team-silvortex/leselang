// This composition imports the public SDK and application-owned modules only.
import { randomUUID } from "node:crypto";
import { fileURLToPath } from "node:url";
import { AgentSession, createRegisteredAgentHost, createSemanticProvider } from "@leselang/bridge";
import { createNodeAgentTransport } from "@leselang/bridge/node";
import { createPublicationApplication } from "./application.mjs";
import { createPublicationAgentCatalog } from "./catalog.mjs";

export const defaultWorker = fileURLToPath(new URL("../../../target/lean/leselang-agent-worker", import.meta.url));

/** The model survives each worker; each execution gets a fresh Rust VM. */
export function createPublicationAgent({
  application = createPublicationApplication(), executable = defaultWorker,
  provider = createSemanticProvider(), onStderr, timeoutMs = 15_000,
} = {}) {
  const transport = createNodeAgentTransport({
    executable, catalog: createPublicationAgentCatalog, timeoutMs, onStderr,
  });
  const host = createRegisteredAgentHost({
    window: "node-agent", epoch: randomUUID(), application, transport,
    observations: { before: "objects_before", after: "objects_after" },
  });
  const session = new AgentSession({ host, provider });
  return { application, transport, host, session,
    close() { session.cancel(); host.close(); },
  };
}
