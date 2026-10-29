import React from "react";
import ReactDOM from "react-dom/client";
import { polyfillCountryFlagEmojis } from "country-flag-emoji-polyfill";
import App from "./App";
import "@/styles/globals.css";
import { i18nReady } from "@/i18n";
import { preloadIcons } from "@/utils/icons";
import { installGlobalErrorLogging } from "@/lib/logger";
import { registerMcpConsumer } from "@/mcp/register";
import { getPlatform } from "@/utils/platform";

// Chromium/WebView2 on Windows can't render country flag emoji natively
// (regional indicator pairs show as letter codes, e.g. "DE"); this loads a
// self-hosted font subset only when the OS lacks native flag support.
polyfillCountryFlagEmojis(undefined, "/fonts/TwemojiCountryFlags.woff2");

preloadIcons();

installGlobalErrorLogging();

registerMcpConsumer();

// Mobile shell attributes must exist before the first paint: `data-android` comes from the
// native shell (NativeFeel.kt), `data-ios` from here (the whole `html:is([data-android],[data-ios])`
// block in globals.css depends on it). getPlatform never rejects, so this can't block startup.
void Promise.all([i18nReady, getPlatform()]).then(([, platform]) => {
  if (platform === "ios") document.documentElement.setAttribute("data-ios", "");
  ReactDOM.createRoot(document.getElementById("root") as HTMLElement).render(
    <React.StrictMode>
      <App />
    </React.StrictMode>,
  );
});
