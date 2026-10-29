/**
 * Haptic feedback for the mobile shells. `haptic(kind)` asks the native side to play the
 * matching system feedback, so the user's system touch-feedback setting applies:
 *   - Android: `window.VoltiusHaptics.perform(kind)` → Haptics.kt View.performHapticFeedback
 *     with the matching HapticFeedbackConstants;
 *   - iOS: `window.webkit.messageHandlers.voltiusHaptics.postMessage({ kind })` → the
 *     WKScriptMessageHandler arms UIImpactFeedbackGenerator / UINotificationFeedbackGenerator.
 * Anywhere without either bridge (desktop, web, tests) it is a no-op, so shared components
 * can call it unconditionally.
 *
 * Keep it sparse: discrete, meaningful moments only (a key, a long-press firing, a
 * switch flipping, a sheet flung away, a destructive confirm, a tab change, an error).
 */

export const HAPTIC_KINDS = [
  /** Extra-keys row key / modifier tap. KEYBOARD_TAP. */
  "key",
  /** Light tick: bottom-nav tab change. CLOCK_TICK. */
  "tick",
  /** A long-press fired (row menu, modifier lock, terminal selection). LONG_PRESS. */
  "press",
  /** A destructive action confirmed. CONFIRM (API 30+). */
  "confirm",
  /** An operation failed (error snackbar). REJECT (API 30+). */
  "reject",
  /** A drag gesture committed (sheet flung / dragged away). GESTURE_END (API 30+). */
  "gesture",
  /** Switch turned on / off. TOGGLE_ON / TOGGLE_OFF (API 34+). */
  "toggleOn",
  "toggleOff",
] as const;
export type HapticKind = (typeof HAPTIC_KINDS)[number];

type Bridge = { perform(kind: string): boolean };
/** WKScriptMessageHandler exposed by the iOS shell. */
type WebkitBridge = { postMessage(message: unknown): void };
type HapticWindow = Window & {
  VoltiusHaptics?: Bridge;
  webkit?: { messageHandlers?: { voltiusHaptics?: WebkitBridge } };
  /** Debug counter: calls that reached the bridge, per kind (read it with adb/CDP). */
  __voltiusHaptics?: { count: number; last: HapticKind | null; byKind: Partial<Record<HapticKind, number>> };
};

/** Play `kind`. Returns true if the native bridge accepted it, false (no-op) elsewhere. */
export function haptic(kind: HapticKind): boolean {
  if (typeof window === "undefined") return false;
  const w = window as HapticWindow;
  const bridge = w.VoltiusHaptics;
  let ok = false;
  if (bridge) {
    try {
      ok = bridge.perform(kind) === true;
    } catch {
      return false;
    }
  } else {
    // iOS: the WKWebView message handler is fire-and-forget, so it counts as accepted.
    const webkit = w.webkit?.messageHandlers?.voltiusHaptics;
    if (!webkit) return false;
    try {
      webkit.postMessage({ kind });
      ok = true;
    } catch {
      return false;
    }
  }
  const stats = (w.__voltiusHaptics ??= { count: 0, last: null, byKind: {} });
  stats.count += 1;
  stats.last = kind;
  stats.byKind[kind] = (stats.byKind[kind] ?? 0) + 1;
  return ok;
}

/** The switch kind for a toggle's new value. */
export const toggleHaptic = (on: boolean): HapticKind => (on ? "toggleOn" : "toggleOff");
