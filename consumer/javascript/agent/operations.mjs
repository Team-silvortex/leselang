import { fileURLToPath } from "node:url";
import { AgentSession } from "@leselang/bridge";
import { createNodeOperationHost } from "@leselang/bridge/node";
import { createOperationRegistry } from "@leselang/bridge/operations";

export const worker = fileURLToPath(new URL("../../../target/lean/leselang-agent-worker", import.meta.url));

// Offline command: select two unary operations, then supply the first one's
// string argument on the next line. Operation and parameter names in the Lese
// source come from the discovered signatures. The input stays ordinary data.
export const pipelineProvider = {
  async plan({ message, catalog }) {
    const command = /^([^\s]+) \|> ([^\s]+)\n([\s\S]*)$/u.exec(message);
    if (!command) throw new Error("Expected FIRST |> SECOND followed by a newline and input text");
    const [first, second] = command.slice(1, 3).map((name) => {
      const schema = catalog.operations.find((op) => op.name === name);
      if (!schema || Object.keys(schema.parameters).length !== 1) {
        throw new Error("The catalog must declare each selected operation with exactly one argument");
      }
      const [[parameter, type]] = Object.entries(schema.parameters);
      return { name: schema.name, parameter, type, result: schema.result };
    });
    if (first.type !== "string" || first.result !== second.type) {
      throw new Error("Operation types do not compose");
    }
    return {
      summary: `Call ${first.name}, then pass its result to ${second.name}`,
      source: `fn main(input: string) = bind(previous: ${first.name}(${first.parameter}: input), body: ${second.name}(${second.parameter}: previous))`,
      arguments: { input: command[3] },
    };
  },
};

// The ledger stays in the application. Its receipt ids and stock values
// intentionally exceed Number.MAX_SAFE_INTEGER to exercise the u64 boundary.
export function createStockAgent(executable = worker) {
  const firstTicket = 9_007_199_254_740_993n;
  let available = 9_007_199_254_740_999n;
  const reservations = new Map();
  const registry = createOperationRegistry([
    { name: "stock.reserve", parameters: { item: "string" }, result: "integer", capability: "stock.write",
      handle({ item }, context) {
        if (available === 0n) throw new Error("Stock exhausted");
        const ticket = firstTicket + BigInt(reservations.size);
        context.commit(() => { reservations.set(ticket, item); available--; });
        return ticket;
      } },
    { name: "stock.remaining", parameters: { ticket: "integer" }, result: "integer", capability: "stock.read",
      handle({ ticket }) {
        if (!reservations.has(ticket)) throw new Error("Unknown reservation");
        return available;
      } },
  ]);
  const host = createNodeOperationHost({
    executable, registry, grants: () => ["stock.read", "stock.write"],
    observe() {
      const count = reservations.size;
      return { count, available: available.toString(), lastItem: reservations.get(firstTicket + BigInt(count) - 1n) ?? null };
    },
    verify({ before, after, plan, report }) {
      return report.effects === 2 && report.result.type === "integer" &&
        report.result.value === after.available && after.count === before.count + 1 &&
        BigInt(after.available) === BigInt(before.available) - 1n &&
        after.lastItem === plan.arguments.input;
    },
  });
  return { session: new AgentSession({ host, provider: pipelineProvider }), close: () => host.close(),
    snapshot: () => ({ available, reservations: [...reservations] }) };
}

if (import.meta.main) {
  const agent = createStockAgent();
  try {
    const outcome = await agent.session.submit("stock.reserve |> stock.remaining\nSpare parts");
    console.log(JSON.stringify(outcome));
    if (outcome.status !== "verified") process.exitCode = 1;
  } finally { agent.close(); }
}
