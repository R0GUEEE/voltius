/**
 * iOS soft-keyboard bridge for the terminal.
 *
 * Unlike Android — where a native overlay owns the IME and forwards clean input through
 * `window.__voltiusTermInput` (see services/androidKeyboard.ts, #34) — on iOS the WKWebView
 * keyboard *is* xterm's IME: the hidden `.xterm-helper-textarea` stays editable (see the
 * platform branch in `useTerminal`), so xterm's own `onData` path handles the input. All the
 * gesture layer needs is a way to raise the keyboard (focus the textarea) and dismiss it.
 */
import { focusSession, getTerminalApi } from "@/hooks/useTerminal";

/** xterm's hidden helper textarea for `sessionId`, or null when the terminal is not mounted. */
function helperTextarea(sessionId: string): HTMLTextAreaElement | null {
  const screen = getTerminalApi(sessionId)?.screenEl();
  const root = screen?.closest(".xterm") ?? null;
  return root?.querySelector<HTMLTextAreaElement>(".xterm-helper-textarea") ?? null;
}

/** Raise the soft keyboard for `sessionId` (idempotent). */
export function showIOSKeyboard(sessionId: string): void {
  const ta = helperTextarea(sessionId);
  if (!ta) {
    // Terminal not in the DOM yet — let xterm's own focus path find its textarea.
    focusSession(sessionId);
    return;
  }
  try {
    ta.focus({ preventScroll: true });
  } catch {
    ta.focus();
  }
}

/** Dismiss the soft keyboard by blurring whatever currently holds focus. */
export function hideIOSKeyboard(): void {
  const el = document.activeElement as HTMLElement | null;
  if (el && el !== document.body && typeof el.blur === "function") el.blur();
}
