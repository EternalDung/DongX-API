import { describe, it, expect, vi } from "vitest";
import { sleep } from "../async";

describe("sleep", () => {
  it("resolves with undefined for 0ms", async () => {
    await expect(sleep(0)).resolves.toBeUndefined();
  });

  it("does not resolve before the requested duration has elapsed", async () => {
    vi.useFakeTimers();
    try {
      let resolved = false;
      const p = sleep(400).then(() => {
        resolved = true;
      });
      // 尚未到 400ms —— 不应 resolve
      expect(resolved).toBe(false);
      await vi.advanceTimersByTimeAsync(399);
      expect(resolved).toBe(false);
      // 越过 400ms —— 应 resolve
      await vi.advanceTimersByTimeAsync(2);
      await p;
      expect(resolved).toBe(true);
    } finally {
      vi.useRealTimers();
    }
  });
});
