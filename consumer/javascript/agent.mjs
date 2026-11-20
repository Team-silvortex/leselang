// An explicit offline workflow demonstration, not a natural-language planner.
import { AgentWorkflowSession, prepareSemanticWorkflow } from "@leselang/bridge";
import { createPublicationAgent } from "./agent/assistant.mjs";
import { createPublicationApplication } from "./agent/application.mjs";

const args = process.argv.slice(2);
let text = "Node 独立应用工作流 ✓", faultAfterCommit = false;
if (args[0] === "--text" && args.length >= 2) { text = args[1]; args.splice(0, 2); }
if (args[0] === "--fault-after-commit") { faultAfterCommit = true; args.shift(); }
if (args.length || Buffer.byteLength(text) > 4096 || text.includes("\0")) {
  process.stderr.write("usage: node agent.mjs [--text TEXT] [--fault-after-commit]\n");
  process.exitCode = 2;
} else {
  const app = createPublicationAgent({
    application: createPublicationApplication({ initialDraft: text, faultAfterCommit }),
    executionReports: "v1",
    onStderr: (chunk) => process.stderr.write(chunk),
  });
  const workflow = new AgentWorkflowSession({
    host: app.host,
    provider: { async plan({ catalog }) {
      return prepareSemanticWorkflow(catalog, [
        { task: "read_draft", arguments: {} },
        { task: "publish_article", arguments: { text: { fromStep: 0, format: "string" } },
          requires: { fromStep: 0, format: "string", equals: text } },
        { task: "read_publication", arguments: {} },
        { task: "check_publication", arguments: {} },
      ]);
    } },
  });
  try {
    const result = await workflow.submit("Run the declared offline publication workflow");
    process.stdout.write(JSON.stringify({ kind: "node_agent_demo", status: result.status,
      possiblePartialEffects: result.possiblePartialEffects, error: result.error,
      verificationFailure: result.verificationFailure,
      steps: result.steps.map((step) => {
        const report = typeof step.report === "string" ? JSON.parse(step.report) : null;
        return { task: step.plan?.task, status: step.status, result: report?.result,
          effects: report?.effects, fuel_remaining: report?.fuel_remaining,
          verificationFailure: step.verificationFailure, report: step.report };
      }),
      application: app.application.snapshot(),
    }) + "\n");
    if (result.status !== "verified") process.exitCode = 1;
  } finally { workflow.cancel(); app.close(); }
}
