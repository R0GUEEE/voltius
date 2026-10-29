import { beforeEach, describe, expect, it, vi } from "vitest";

const invoke = vi.fn();
vi.mock("@tauri-apps/api/core", () => ({ invoke: (...args: unknown[]) => invoke(...args) }));

describe("getPlatformSync", () => {
  beforeEach(() => {
    vi.resetModules();
    invoke.mockReset();
  });

  it("is null before the platform resolves", async () => {
    invoke.mockReturnValue(new Promise(() => {}));
    const { getPlatform, getPlatformSync } = await import("./platform");
    getPlatform();
    expect(getPlatformSync()).toBeNull();
  });

  it("returns the resolved platform once known", async () => {
    invoke.mockResolvedValue("android");
    const { getPlatform, getPlatformSync } = await import("./platform");
    await getPlatform();
    expect(getPlatformSync()).toBe("android");
  });

  it("reports the fallback when the backend call fails", async () => {
    invoke.mockRejectedValue(new Error("no backend"));
    const { getPlatform, getPlatformSync } = await import("./platform");
    await getPlatform();
    expect(getPlatformSync()).toBe("unknown");
  });
});

describe("isMobileOS", () => {
  it("is true for android and ios only", async () => {
    const { isMobileOS } = await import("./platform");
    expect(isMobileOS("android")).toBe(true);
    expect(isMobileOS("ios")).toBe(true);
    expect(isMobileOS("macos")).toBe(false);
    expect(isMobileOS("linux")).toBe(false);
    expect(isMobileOS("windows")).toBe(false);
    expect(isMobileOS("unknown")).toBe(false);
    expect(isMobileOS(null)).toBe(false);
  });
});

describe("isMobileShell", () => {
  beforeEach(() => {
    vi.resetModules();
    invoke.mockReset();
  });

  it("is false before the platform resolves", async () => {
    invoke.mockReturnValue(new Promise(() => {}));
    const { getPlatform, isMobileShell } = await import("./platform");
    getPlatform();
    expect(isMobileShell()).toBe(false);
  });

  it("is true on android", async () => {
    invoke.mockResolvedValue("android");
    const { getPlatform, isMobileShell } = await import("./platform");
    await getPlatform();
    expect(isMobileShell()).toBe(true);
  });

  it("is true on ios", async () => {
    invoke.mockResolvedValue("ios");
    const { getPlatform, isMobileShell } = await import("./platform");
    await getPlatform();
    expect(isMobileShell()).toBe(true);
  });

  it("is false on desktop", async () => {
    invoke.mockResolvedValue("macos");
    const { getPlatform, isMobileShell } = await import("./platform");
    await getPlatform();
    expect(isMobileShell()).toBe(false);
  });
});
