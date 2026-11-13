import type { AgentProvider, WorkflowPlan } from "./agent.js";
import { prepareSemanticCatalog } from "./semantics.js";

/** Application-owned command aliases; task meanings always come from the catalog. */
export interface SemanticProviderOptions {
  readonly aliases?: Readonly<Record<string, string>>;
}

const identifier = /^[A-Za-z_][A-Za-z0-9_]{0,63}$/u;
const encoder = new TextEncoder();

function text(value: string): string {
  if (value.includes("\0") || encoder.encode(value).length > 4096) {
    throw new Error("任务输入超过 4096 UTF-8 字节或含有无效字符。");
  }
  for (let index = 0; index < value.length; index += 1) {
    const unit = value.charCodeAt(index);
    if (unit >= 0xd800 && unit <= 0xdbff) {
      const next = value.charCodeAt(++index);
      if (!(next >= 0xdc00 && next <= 0xdfff)) throw new Error("任务输入含有无效字符。");
    } else if (unit >= 0xdc00 && unit <= 0xdfff) {
      throw new Error("任务输入含有无效字符。");
    }
  }
  return value;
}

/** Deterministic offline selection; no model call, execution or retry. */
export function createSemanticProvider(options: SemanticProviderOptions = {}): AgentProvider {
  const aliases = new Map<string, string>();
  for (const [alias, task] of Object.entries(options.aliases ?? {})) {
    if (!alias || alias.length > 64 || /[\s:：\0]/u.test(alias) || !identifier.test(task)) {
      throw new Error("invalid_semantic_provider_alias");
    }
    aliases.set(alias, task);
  }
  return {
    async plan(context, signal): Promise<WorkflowPlan> {
      if (signal.aborted) throw new Error("任务已取消。");
      if (typeof context.message !== "string" || encoder.encode(context.message).length > 8192) {
        throw new Error("任务消息格式无效或超过 8192 UTF-8 字节。");
      }
      const catalog = prepareSemanticCatalog(context.catalog);
      const explicit = /^(?:task|任务) ([A-Za-z_][A-Za-z0-9_]{0,63})(?:[:：]([\s\S]*))?$/u.exec(context.message);
      let task = explicit?.[1];
      let input = explicit?.[2];
      if (!explicit) {
        const alias = /^([^:：\s]+)[:：]([\s\S]*)$/u.exec(context.message);
        if (alias) {
          task = aliases.get(alias[1]!);
          input = alias[2];
        }
      }
      if (!task) throw new Error("请使用目录中的任务命令：task 任务名:文本；无输入任务使用 task 任务名。");
      const declaration = catalog.tasks.find((candidate) => candidate.id === task);
      if (!declaration) throw new Error("宿主目录没有声明这个任务。");
      if ((declaration.inputName !== undefined) !== (input !== undefined)) {
        throw new Error(declaration.inputName === undefined ? "这个任务不接受文本输入。" : "这个任务需要冒号后的文本输入。");
      }
      const arguments_: Record<string, string> = Object.create(null);
      if (declaration.inputName !== undefined) arguments_[declaration.inputName] = text(input!);
      const prepared = catalog.prepare(task, arguments_);
      if (signal.aborted) throw new Error("任务已取消。");
      return prepared.plan;
    },
  };
}
