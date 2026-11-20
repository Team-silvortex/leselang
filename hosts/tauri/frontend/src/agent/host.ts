import { createRegisteredAgentHost, type RegisteredApplicationConnection, type RegisteredAgentHost } from "../bridge.js";
import type { TauriApi } from "../tauri-api.js";

/** Kept as a local compatibility name for the reference applications. */
export type ApplicationConnection = RegisteredApplicationConnection;

/** Tauri only carries messages; the SDK owns registered-application lifecycle and evidence. */
export function createAgentHost(api: TauriApi, application: ApplicationConnection, options: {
  epoch: string;
  onSource?: (source: string) => void;
}): RegisteredAgentHost {
  return createRegisteredAgentHost({
    window: "main", epoch: options.epoch, application, onSource: options.onSource,
    executionReports: "v1",
    observations: { before: "native_before", after: "native_after" },
    transport: {
      describe: () => api.core.invoke("agent_describe", { raw: true, taskCatalog: true, observedResults: true }),
      execute(run) {
        const channel = new api.core.Channel<unknown>();
        channel.onmessage = run.onRequest;
        if (run.inputMode !== undefined && run.inputMode !== "named_strings") throw new Error("invalid_task_input_mode");
        return api.core.invoke(run.inputMode === "named_strings" ? "agent_start_named_task" : "agent_start_task", {
          onRequest: channel, epoch: run.scope.epoch, session: run.scope.session,
          source: run.source, arguments: run.arguments,
        });
      },
      reply: (reply) => api.core.invoke<void>("agent_reply", { reply }),
      cancel: (scope) => api.core.invoke<void>("agent_cancel", { epoch: scope.epoch, session: scope.session }),
      close: (endpoint) => api.core.invoke<void>("agent_closed", { epoch: endpoint.epoch }),
    },
  });
}
