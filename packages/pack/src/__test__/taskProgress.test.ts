import { afterEach, describe, expect, it, vi } from "vitest";
import {
  formatDuration,
  formatTaskCount,
  TaskProgress,
} from "../utils/taskProgress";

const originalIsTTY = process.stdout.isTTY;

function setStdoutIsTTY(value: boolean) {
  Object.defineProperty(process.stdout, "isTTY", {
    configurable: true,
    value,
  });
}

afterEach(() => {
  vi.useRealTimers();
  vi.restoreAllMocks();
  setStdoutIsTTY(originalIsTTY);
});

describe("TaskProgress", () => {
  it("logs once outside a TTY and returns the completed task delta", () => {
    setStdoutIsTTY(false);
    const log = vi.spyOn(console, "log").mockImplementation(() => {});
    let completed = 10;
    const progress = new TaskProgress({
      getCompletedTaskCount: () => completed,
    });

    progress.start("Compiling");
    completed = 13;

    expect(progress.stop()).toBe(3);
    expect(log).toHaveBeenCalledOnce();
    expect(log).toHaveBeenCalledWith("Compiling... (0 tasks completed)");
  });

  it("refreshes the completed task count in a TTY", () => {
    vi.useFakeTimers();
    setStdoutIsTTY(true);
    const write = vi
      .spyOn(process.stdout, "write")
      .mockImplementation(() => true);
    let completed = 4;
    const progress = new TaskProgress({
      getCompletedTaskCount: () => completed,
    });

    progress.start("Compiling");
    completed = 7;
    vi.advanceTimersByTime(100);

    expect(write.mock.calls.map(([value]) => String(value))).toContain(
      "\rCompiling... (3 tasks completed)",
    );
    expect(progress.stop()).toBe(3);
  });

  it("handles the u32 counter wrapping", () => {
    setStdoutIsTTY(false);
    vi.spyOn(console, "log").mockImplementation(() => {});
    let completed = 0xffff_ffff;
    const progress = new TaskProgress({
      getCompletedTaskCount: () => completed,
    });

    progress.start("Compiling");
    completed = 1;

    expect(progress.stop()).toBe(2);
  });

  it("formats durations and task counts", () => {
    expect(formatDuration(2_000)).toBe("2000ms");
    expect(formatDuration(2_100)).toBe("2.1s");
    expect(formatTaskCount(1)).toBe("1 task");
    expect(formatTaskCount(2)).toBe("2 tasks");
  });
});
