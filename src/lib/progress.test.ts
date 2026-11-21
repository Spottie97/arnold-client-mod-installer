import { describe, expect, it } from "vitest";
import { mergeProgress } from "./progress";
import type { ProgressEvent } from "../types";

function event(
  partial: Partial<ProgressEvent> & Pick<ProgressEvent, "step" | "status">,
): ProgressEvent {
  return {
    runId: "run-1",
    step: partial.step,
    file: partial.file,
    status: partial.status,
    percent: partial.percent ?? 0,
    message: partial.message ?? partial.step,
  };
}

describe("mergeProgress", () => {
  it("replaces the last running row when step, file, and status match", () => {
    const start = event({
      step: "download",
      status: "running",
      file: "lootr.jar",
      percent: 13,
      message: "Downloading Lootr",
    });
    const tick = event({
      step: "download",
      status: "running",
      file: "lootr.jar",
      percent: 14,
      message: "Downloading Lootr — 20.0/40.0 MB",
    });
    const rows = [start];
    expect(mergeProgress(rows, tick)).toEqual([tick]);
    expect(rows).toEqual([start]);

    const prepare = event({ step: "prepare", status: "running", percent: 2, message: "Preparing" });
    const prepareTick = event({
      step: "prepare",
      status: "running",
      percent: 3,
      message: "Preparing install",
    });
    expect(mergeProgress([prepare], prepareTick)).toEqual([prepareTick]);

    const other = event({ step: "download", status: "running", file: "a.jar", message: "a" });
    const current = event({ step: "download", status: "running", file: "b.jar", message: "b" });
    const currentTick = event({
      step: "download",
      status: "running",
      file: "b.jar",
      message: "b2",
      percent: 40,
    });
    expect(mergeProgress([other, current], currentTick)).toEqual([other, currentTick]);
  });

  it("appends when the step, file, or status changes", () => {
    const row = event({ step: "download", status: "running", file: "a.jar", percent: 10 });
    const step = event({ step: "write", status: "running", file: "a.jar", percent: 80 });
    const file = event({ step: "download", status: "running", file: "b.jar", percent: 20 });
    const status = event({ step: "download", status: "done", file: "a.jar", percent: 10 });
    expect(mergeProgress([row], step)).toEqual([row, step]);
    expect(mergeProgress([row], file)).toEqual([row, file]);
    expect(mergeProgress([row], status)).toEqual([row, status]);
    expect(mergeProgress([], row)).toEqual([row]);
  });

  it("appends terminal rows", () => {
    const running = event({ step: "download", status: "running", file: "a.jar", percent: 40 });
    const done = event({ step: "done", status: "done", percent: 100, message: "Install finished" });
    const cancelled = event({ step: "rollback", status: "cancelled", percent: 100 });
    const error = event({ step: "error", status: "error", percent: 100 });
    expect(mergeProgress([running], done)).toEqual([running, done]);
    expect(mergeProgress([running], cancelled)).toEqual([running, cancelled]);
    expect(mergeProgress([running], error)).toEqual([running, error]);
    const doneAgain = event({ step: "done", status: "done", percent: 100, message: "again" });
    expect(mergeProgress([done], doneAgain)).toEqual([done, doneAgain]);
  });
});
