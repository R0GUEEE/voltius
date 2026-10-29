import { beforeEach, describe, expect, it, vi } from "vitest";

const h = vi.hoisted(() => ({
  invoke: vi.fn(() => Promise.resolve()),
  platform: "android",
}));
vi.mock("@/lib/invoke", () => ({ invoke: h.invoke }));
vi.mock("@/utils/platform", () => ({
  getPlatform: () => Promise.resolve(h.platform),
  isMobileOS: (p: string | null) => p === "android" || p === "ios",
}));

import { resetSystemBarsForTest, syncSystemBars } from "./systemBars";

const flush = () => new Promise((r) => setTimeout(r, 0));

describe("systemBars", () => {
  beforeEach(() => {
    h.invoke.mockClear();
    h.platform = "android";
    resetSystemBarsForTest();
  });

  it("decides from background luminance, not theme name", async () => {
    for (const bg of ["#0f1729", "#f5f5f0", "not-a-color", "#ffffff"]) {
      syncSystemBars(bg);
      await flush();
    }
    expect(h.invoke.mock.calls.map((c) => (c as unknown[])[1])).toEqual([
      { lightBackground: false },
      { lightBackground: true },
      { lightBackground: false },
      { lightBackground: true },
    ]);
  });

  it("pushes the verdict on Android and dedupes repeats", async () => {
    syncSystemBars("#0f1729");
    await flush();
    syncSystemBars("#101a2e");
    await flush();
    syncSystemBars("#fafafa");
    await flush();
    expect(h.invoke.mock.calls).toEqual([
      ["set_system_bars", { lightBackground: false }],
      ["set_system_bars", { lightBackground: true }],
    ]);
  });

  it("pushes on iOS too", async () => {
    h.platform = "ios";
    syncSystemBars("#0f1729");
    await flush();
    expect(h.invoke.mock.calls).toEqual([["set_system_bars", { lightBackground: false }]]);
  });

  it("never invokes on desktop", async () => {
    h.platform = "windows";
    syncSystemBars("#ffffff");
    await flush();
    expect(h.invoke).not.toHaveBeenCalled();
  });
});
