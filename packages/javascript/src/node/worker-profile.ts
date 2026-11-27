export interface WorkerPhases {
  readonly compile_ns: number;
  readonly vm_local_ns: number;
  readonly exchange_ns: number;
  readonly exchange_count: number;
}

export interface NodeGuiWorkerProfile {
  readonly wall_ns: number;
  readonly startup_ns: number;
  /** Synchronous spawn() duration, included in startup_ns. */
  readonly spawn_call_ns: number;
  readonly shutdown_ns: number;
  readonly request_count: number;
  readonly worker_profile: WorkerPhases | null;
}

/** One host clock per profiled child. Worker fields are elapsed durations on its own clock. */
export class GuiWorkerClock {
  private readonly start = process.hrtime.bigint();
  private spawnReturnedAt?: bigint;
  readyAt?: bigint;
  private stopAt?: bigint;
  private worker: WorkerPhases | null = null;

  spawnReturned(): void {
    this.spawnReturnedAt = process.hrtime.bigint();
  }

  ready(): void {
    if (this.readyAt !== undefined) throw new Error("node_worker_duplicate_ready");
    this.readyAt = process.hrtime.bigint();
  }

  report(fields: Record<string, unknown>, requestCount: number): void {
    const { compile_ns, vm_local_ns, exchange_ns, exchange_count } = fields;
    const duration = (v: unknown): v is number => typeof v === "number" && Number.isSafeInteger(v) && v >= 0;
    if (!duration(compile_ns) || !duration(vm_local_ns) || !duration(exchange_ns) || exchange_count !== requestCount) {
      throw new Error("node_worker_invalid_profile");
    }
    this.worker = { compile_ns, vm_local_ns, exchange_ns, exchange_count: requestCount };
    this.stopAt = process.hrtime.bigint();
  }

  cancel(): void {
    this.stopAt = process.hrtime.bigint();
    this.worker = null;
  }

  finish(close: bigint, requestCount: number): NodeGuiWorkerProfile {
    if (this.spawnReturnedAt === undefined || this.readyAt === undefined || this.stopAt === undefined) {
      throw new Error("node_worker_incomplete_profile");
    }
    return {
      wall_ns: Number(close - this.start),
      startup_ns: Number(this.readyAt - this.start),
      spawn_call_ns: Number(this.spawnReturnedAt - this.start),
      shutdown_ns: Number(close - this.stopAt),
      request_count: requestCount,
      worker_profile: this.worker,
    };
  }
}
