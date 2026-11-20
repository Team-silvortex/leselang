/** Display an already verified report; this helper never establishes success. */
export function describeVerifiedResult(raw: unknown): string {
  if (typeof raw !== "string") return "结果见原始报告";
  try {
    const result: unknown = JSON.parse(raw).result;
    if (!result || typeof result !== "object" || Array.isArray(result)) return "结果见原始报告";
    const { type, value } = result as { type?: unknown; value?: unknown };
    switch (type) {
      case "integer":
        // The host verifier uses the lossless semantic parser. A UI JSON parse
        // must never display a rounded u64 as the actual verified return value.
        return Number.isSafeInteger(value) ? `integer：${value}` : "integer：精确整数，见原始报告";
      case "boolean":
        if (typeof value === "boolean") return `boolean：${value}`;
        break;
      case "string":
        if (typeof value === "string") return `string：${JSON.stringify(value)}`;
        break;
      case "none":
        if (value === null) return "none";
        break;
      case "optional_string":
        if (value === null || typeof value === "string") return `optional_string：${JSON.stringify(value)}`;
        break;
      case "string_list":
        if (Array.isArray(value) && value.every((item) => typeof item === "string")) {
          return `string_list：${JSON.stringify(value)}`;
        }
        break;
    }
  } catch { /* A missing display value cannot upgrade the task's outcome. */ }
  return "结果见原始报告";
}
