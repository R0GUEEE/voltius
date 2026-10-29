# Voltius on iOS

The iOS target is a first-class mobile port: the same Rust core, the same React
frontend, rendered by `MobileShell` exactly as on Android. It is a **sideload**
target — there is no App Store submission here and no signing identity in the
repo.

```
# macOS host with Xcode 16+ (or the `ios` GitHub workflow)
pnpm install
pnpm build            # tsc + seeded plugin bundles + vite dist
pnpm ios:init         # xcodegen generates src-tauri/gen/apple from project.yml
pnpm ios:dev          # run on a simulator /device
pnpm ios:build        # signed build (needs bundle.iOS.developmentTeam)
```

`gen/apple/` is gitignored and machine-specific: the generated project embeds the
absolute path of the `tauri` CLI that created it, so it is produced per build.

## The unsigned `.ipa`

`.github/workflows/ios.yml` builds an **unsigned** IPA on a `macos-15` runner:

1. `pnpm install --frozen-lockfile && pnpm build`
2. `pnpm tauri ios init --ci` → `xcodegen` generates `gen/apple`
3. `xcodebuild -configuration Release -sdk iphoneos CODE_SIGNING_ALLOWED=NO`
   (the generated *Build Rust Code* phase runs
   `cargo build --target aarch64-apple-ios --release`)
4. `Payload/Voltius.app` → `zip -D` → `Voltius-ios-unsigned.ipa`

Download the `voltius-ios-ipa` artifact and re-sign it with SideStore, AltStore,
Sideloadly or TrollStore. An unsigned IPA cannot be installed as-is — iOS
requires a signature, and the re-signer supplies it.

`.github/workflows/ios-check.yml` is the fast gate: `cargo check --target
aarch64-apple-ios` on every push that touches `src-tauri/**`.

## What had to be built

| Area | Change |
|---|---|
| Rust backend | `system_auth/apple.rs` (LAContext) now serves macOS **and** iOS; one backend module instead of a macOS-only one. |
| Keychain | `apple-native-keyring-store`'s **data-protection** store (`protected` feature) registered in `init_keychain_store`. The classic macOS "keychain" store does not exist on iOS. |
| Storage | `storage::config::set_config_dir` is pinned to `app_data_dir()/voltius` on iOS as well as Android, so connections/identities/plugins and the vault live in one tree (`dirs::config_dir()` resolves to nothing on iOS). |
| Machine fingerprint | `UIDevice.identifierForVendor` via the ObjC runtime (no `objc2-ui-kit` dependency) replaces the `TODO(ios)`. |
| Info.plist | `src-tauri/Info.ios.plist`, merged by `bundle.iOS.infoPlist`: local-network usage (iOS 14+ refuses LAN sockets without it), Face ID, Files.app visibility, export compliance. |
| Frontend | `isMobileOS()` / `useIsMobile()` make iOS a mobile shell; the Android-only bridges (IME overlay, SAF downloads, volume-key ctrl, Material You) stay Android-only, and iOS gets its own paths (xterm textarea keyboard, save dialog/share for downloads, status-bar contrast). `html[data-ios]` mirrors the 48 `html[data-android]` mobile CSS rules. |

## Support matrix

| Feature | iOS |
|---|---|
| SSH, SFTP, port forwarding, jump hosts | ✅ BSD sockets, no sandbox problem |
| SOCKS/HTTP proxy, system proxy detection | ✅ (manual config; iOS has no `scutil`) |
| Vault, secrets, E2EE sync (gist/Cloudflare/S3) | ✅ data-protection keychain — requires a **signed** build with a provisioning profile |
| Remote Docker, Proxmox, process list, metrics | ✅ network-only |
| System auth (Face ID / passcode) app lock | ✅ `LAContext` |
| Plugins (JS) | ✅ |
| Local terminal (PTY + shell) | ❌ no `fork`/`exec`, no `openpty` in the iOS sandbox — the shell picker hides local shells on mobile |
| Serial console | ❌ no `/dev/tty*`, needs the USB accessory API |
| Local Docker (`/var/run/docker.sock`) | ❌ use a TCP/SSH endpoint |
| MCP server (unix socket) | ❌ single-app sandbox, no external client can reach it |
| Background SSH sessions | ❌ iOS suspends the process ~30 s after backgrounding |
| Haptics / share sheet / hardware-back helpers | ⚠️ frontend already speaks the `WKWebView` message-handler contract (`voltiusHaptics`, `voltiusShare`, `voltiusBack`); the native handlers are the next piece of work |

## Follow-ups

- **Native bridge module** — a small ObjC/Swift target registering
  `voltiusHaptics` (`UIImpactFeedbackGenerator`), `voltiusShare`
  (`UIActivityViewController`) and `voltiusBack` (edge-swipe →
  `window.__voltiusBack`). The frontend is already written against it: the
  bridges are feature-detected, so adding them lights the features up with no
  frontend change.
- **Status bar contrast** — `set_system_bars` is a no-op on iOS; it needs
  `preferredStatusBarStyle` on the host view controller.
- **Custom XcodeGen template** — `bundle.iOS.template` points at a `project.yml`
  so the native module above can be committed instead of generated.
- **Background SSH** — `UIBackgroundModes` + `beginBackgroundTask` would buy a
  few minutes; anything long-lived needs a foreground service equivalent, which
  iOS does not have for this class of app.
