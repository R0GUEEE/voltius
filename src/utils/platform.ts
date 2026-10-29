import { useEffect, useState } from "react";
import { invoke } from "@/lib/invoke";

/**
 * Host OS reported by the Rust backend (`get_platform`): "android" | "ios" |
 * "linux" | "macos" | "windows". Resolved once and cached for the session.
 *
 * Used to hide host-integration features the platform sandbox can't support
 * (local terminal, serial, local Docker). This is UX gating only — the backend
 * already fails those operations cleanly; never rely on it for security.
 */
let cached: Promise<string> | null = null;
let resolved: string | null = null;

export function getPlatform(): Promise<string> {
  if (!cached) {
    cached = invoke<string>("get_platform")
      .catch(() => "unknown")
      .then((p) => {
        resolved = p;
        return p;
      });
  }
  return cached;
}

/** The platform if `get_platform` has already resolved, else null. For sync
 *  callers (the plugin runtime) that cannot await; never a security gate. */
export function getPlatformSync(): string | null {
  return resolved;
}

/** True for the touch-first mobile operating systems (Android and iOS) — the
 *  platforms where App renders MobileShell and the host sandbox limits local
 *  integrations. Sync, for non-React callers; false until `get_platform`
 *  resolves, so prime it with `await getPlatform()` first where a wrong answer
 *  on the first pass matters. */
export const isMobileOS = (p: string | null): boolean => p === "android" || p === "ios";

/** True when App is rendering MobileShell rather than DesktopShell. Sync, for
 *  non-React callers; false until `get_platform` resolves, so prime it with
 *  `await getPlatform()` first where a wrong answer on the first pass matters. */
export function isMobileShell(): boolean {
  return isMobileOS(getPlatformSync());
}

/** React hook: the OS string, or `null` until it resolves. */
export function usePlatform(): string | null {
  const [os, setOs] = useState<string | null>(null);
  useEffect(() => {
    let alive = true;
    getPlatform().then((p) => alive && setOs(p));
    return () => {
      alive = false;
    };
  }, []);
  return os;
}

/** True only once the platform is confirmed Android (false while loading). Use
 *  this for Android-only native bridges; for "any mobile OS" gating use
 *  `useIsMobile`. */
export function useIsAndroid(): boolean {
  return usePlatform() === "android";
}

/** True once the platform is a mobile OS — Android or iOS (false while loading). */
export function useIsMobile(): boolean {
  return isMobileOS(usePlatform());
}
