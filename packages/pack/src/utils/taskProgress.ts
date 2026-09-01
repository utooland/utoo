import type { Project } from "../core/types";

const UPDATE_INTERVAL_MS = 100;

export class TaskProgress {
  private baseline: number;
  private label: string | undefined;
  private timer: NodeJS.Timeout | undefined;
  private lastCompleted = -1;
  private lastLineLength = 0;

  constructor(
    private readonly project: Pick<Project, "getCompletedTaskCount">,
  ) {
    this.baseline = project.getCompletedTaskCount();
  }

  start(label: string): void {
    this.label = label;
    this.lastCompleted = -1;
    this.render();

    if (process.stdout.isTTY) {
      this.timer = setInterval(() => this.render(), UPDATE_INTERVAL_MS);
      this.timer.unref();
    }
  }

  stop(): number {
    const current = this.project.getCompletedTaskCount();
    const completed = (current - this.baseline) >>> 0;

    if (this.timer) {
      clearInterval(this.timer);
      this.timer = undefined;
    }

    if (this.label && process.stdout.isTTY) {
      process.stdout.write(`\r${" ".repeat(this.lastLineLength)}\r`);
    }

    this.label = undefined;
    this.lastCompleted = -1;
    this.lastLineLength = 0;
    this.baseline = current;

    return completed;
  }

  private render(): void {
    if (!this.label) {
      return;
    }

    const current = this.project.getCompletedTaskCount();
    const completed = (current - this.baseline) >>> 0;

    if (completed === this.lastCompleted) {
      return;
    }

    this.lastCompleted = completed;
    const message = `${this.label}... (${formatTaskCount(completed)} completed)`;

    if (process.stdout.isTTY) {
      process.stdout.write(`\r${message.padEnd(this.lastLineLength, " ")}`);
      this.lastLineLength = message.length;
    } else {
      console.log(message);
    }
  }
}

export function formatDuration(durationMs: number): string {
  return durationMs > 2_000
    ? `${Math.round(durationMs / 100) / 10}s`
    : `${durationMs}ms`;
}

export function formatTaskCount(taskCount: number): string {
  return `${taskCount} ${taskCount === 1 ? "task" : "tasks"}`;
}
