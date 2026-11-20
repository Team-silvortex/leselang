import { createModelProvider, createModelWorkflowProvider, type AgentProvider, type AgentWorkflowProvider, type ModelProviderOptions } from "./bridge.js";
import type { TauriApi } from "./tauri-api.js";

export interface ModelInfo { readonly configured: boolean; readonly model?: string }

/** Configuration is owned by Rust. Neither credentials nor the endpoint cross IPC. */
export async function modelInfo(api: TauriApi): Promise<ModelInfo> {
  const value = await api.core.invoke<unknown>("agent_model_info");
  if (!value || typeof value !== "object" || Array.isArray(value)) throw new Error("invalid_model_info");
  const info = value as Record<string, unknown>;
  if (typeof info.configured !== "boolean" || Object.keys(info).some((key) => !["configured", "model"].includes(key))
    || (info.configured && (typeof info.model !== "string" || !info.model.trim() || new TextEncoder().encode(info.model).length > 128
      || /[\u0000-\u001f\u007f]/u.test(info.model)))
    || (!info.configured && info.model !== undefined)) throw new Error("invalid_model_info");
  return Object.freeze({ configured: info.configured, ...(info.configured ? { model: info.model as string } : {}) });
}

/** Cancellation signals native work but holds the planning slot until it settles. */
export function createTauriModelProvider(api: TauriApi, options: { epoch: string }): AgentProvider {
  return createModelProvider({ complete: nativeCompletion(api, options.epoch) });
}

/** A single model call may select a bounded sequence of declared tasks. */
export function createTauriModelWorkflowProvider(api: TauriApi, options: { epoch: string }): AgentWorkflowProvider {
  return createModelWorkflowProvider({ complete: nativeCompletion(api, options.epoch) });
}

function nativeCompletion(api: TauriApi, epoch: string): ModelProviderOptions["complete"] {
  let active = false;
  return async (request, signal) => {
    if (signal.aborted) throw new Error("任务已取消。");
    if (active) throw new Error("模型正在处理另一个任务。");
    const requestId = crypto.randomUUID();
    active = true;
    const cancel = () => {
      void Promise.resolve().then(() => api.core.invoke("agent_model_cancel", { epoch, requestId })).catch(() => {});
    };
    signal.addEventListener("abort", cancel, { once: true });
    try {
      const result = await api.core.invoke<unknown>("agent_model_complete", {
        epoch, requestId, request,
      });
      if (signal.aborted) throw new Error("任务已取消。");
      if (typeof result !== "string") throw new Error("invalid_model_response");
      return result;
    } catch {
      // Remote bodies, configuration and transport errors are never UI text.
      throw new Error(signal.aborted ? "任务已取消。" : "模型请求未完成。请检查宿主的模型配置或服务状态；本次没有执行应用操作。");
    } finally {
      signal.removeEventListener("abort", cancel);
      active = false;
    }
  };
}
