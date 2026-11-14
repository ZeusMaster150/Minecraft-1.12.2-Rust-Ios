# iOS build (iPhone 6, iOS 12.5.8)

This document covers the iOS port that is wired into `.github/workflows/ios-build.yml`.
It targets **arm64 iOS 12.0+**, which is what an iPhone 6 running 12.5.8 provides.

## What the port changes and why

| Topic | Desktop | iOS |
| --- | --- | --- |
| Vulkan loader | linked from the system loader (`ash` feature `linked`) | bundled `MoltenVK.framework`, dlopen-ed by `src/vulkan/VulkanEntry.rs` (`ash` feature `loaded`) |
| Shaders | precompiled SPIR-V in `src/vulkan/shaders/spv` (`tools/compile-shaders.sh`) | same checked-in SPIR-V |
| OpenGL backend | available | not available (OpenGL 3.3 compatibility profile does not exist on iOS) |
| Window/event loop | winit on Windows/X11 | winit's UIKit backend, which calls `UIApplicationMain` itself |
| Assets | `runtime/assets` | `Documents/assets` inside the app sandbox |
| Logs | stderr | `Documents/latest.log` |
| Input | keyboard + mouse | touch is translated to the same mouse events in `Minecraft::window_event` |

MoltenVK **must stay on 1.2.7**: releases from 1.2.8 onward raise the minimum to
iOS 13, which an iPhone 6 (iOS 12.5.8) cannot run. 1.2.7 also exposes
`VK_KHR_portability_enumeration`, which `VulkanWindow` enables on iOS.

## 1. Build the IPA

GitHub Actions is the only supported build host: the app must be linked against
the iPhoneOS SDK, which requires macOS + Xcode.

1. Push to `main` (or run **Actions → Build iOS IPA → Run workflow**).
2. When the run finishes, download the `Minecraft-1.12.2-iOS` artifact.
   It contains `Minecraft-1.12.2-iOS.ipa`.

The workflow packages the `.app`, adds the bundled licenses
(`THIRD_PARTY_NOTICES/`), then *fakesigns* the executable and the MoltenVK
framework with `ldid -S platform/ios/entitlements.plist`. That adds the
`application-identifier` and `keychain-access-groups` entitlements iOS needs, so
the same IPA works both on a jailbroken device (Filza / `ipainstaller`) and with
Sideloadly, which replaces the signature with your own Apple ID anyway.

Microsoft sign-in needs an Azure application (client) id; see
[`accounts.md`](accounts.md). You can bake it into the build by adding the
repository variable `MC112_MS_CLIENT_ID`, or drop
`Documents/config/ms-client-id.txt` into the installed app later.

## 2. Sign and install

### Sideloadly / AltStore (no jailbreak)

1. Install [Sideloadly](https://sideloadly.io/) (Windows/macOS) and iTunes (Windows).
2. Connect the iPhone 6, drag the `.ipa` into Sideloadly.
3. Enter your Apple ID. A free Apple ID works, but the app expires after 7 days
   and must be re-signed; a paid developer account extends that to 1 year.
4. Trust the developer certificate on the device:
   `Settings → General → Device Management`.

### Jailbroken iPhone 6

`checkra1n` supports the iPhone 6 on iOS 12.5.8. With AppSync Unified installed,
the IPA can be installed directly in Filza or with `ipainstaller`.

If Filza reports **"application is missing the application-identifier
entitlement"** you installed a build from before the `ldid` step existed (or
AppSync Unified is missing). Install the current artifact, and check that
AppSync Unified is enabled for the iOS version in use.

## 3. Install the Minecraft 1.12.2 assets

The repository does not ship Mojang assets, and the iOS build cannot use
`runtime/assets` from a PC. The client reads them from:

```
Documents/assets/minecraft/...
```

Get the files in one of two ways:

* **From a PC that owns the game:** run `Import-Assets-OneClick.cmd` once so
  `runtime/assets` exists.
* **Without a PC:** `python3 tools/download-assets.py --zip assets.zip` rebuilds
  the official 1.12.2 tree from Mojang's public CDN (see the script header), on
  any machine with Python 3, including Termux.

Then copy them into the app:

1. Connect the device and open the app's Documents folder:
   * macOS 10.15+: Finder → iPhone → Files → MC 1.12.2 Rust;
   * Windows: iTunes → device → File Sharing → MC 1.12.2 Rust;
   * iPhone-only: an app that can unzip (Documents by Readdle) plus the Files
     app, or the Wi-Fi route in `accounts.md`'s sibling notes.
2. Copy the **contents** of `runtime/assets` (`minecraft/` and friends) into
   `Documents/assets/` (for a ZIP, `assets.zip` contains an `assets/` folder:
   unzip it and keep `minecraft/`).
3. Sanity check: `Documents/assets/minecraft/lang/en_us.lang` must exist.

If assets are missing, the client logs the exact expected path to
`Documents/latest.log` and exits; the same file holds renderer/network logs.

## 4. Current limitations

These are real gaps, not packaging problems:

* **No on-screen movement controls.** Touch already maps to mouse press/drag/
  release (`WindowEvent::Touch` → `MouseInput`/`CursorMoved`), so menus, looking
  around and attacking work, but walking needs keys. A virtual joystick plus
  action buttons still has to be added on top of `GameSettings` key bindings.
* **No sound.** `rodio` is a Windows-only dependency of `AudioBackend`.
* **Performance.** An A8 GPU with 1 GB RAM running MoltenVK is slow; the iOS
  defaults already reduce render distance, mipmaps, clouds and entity shadows.
* **Assets are manual.** There is no in-app importer yet.
* **MoltenVK translates SPIR-V to Metal at runtime**, so first launch is slow.
* **Only the Vulkan backend is compiled**; the OpenGL/shader-pack path is
  desktop-only.

## 5. Troubleshooting

| Symptom | Check |
| --- | --- |
| App closes immediately after launch | `Documents/latest.log`, look for "failed to load MoltenVK" or the asset path message |
| Black screen, no log file | the IPA signature is invalid, or `Frameworks/MoltenVK.framework` was not embedded |
| Letterboxed 320×480 window | `LaunchScreen.storyboardc` missing from the `.app` |
| "missing the application-identifier entitlement" (Filza) | build predates the `ldid` step, or AppSync Unified is not enabled |
| Microsoft sign-in says no application id | add `Documents/config/ms-client-id.txt`, see `accounts.md` |
| "Minecraft 1.12.2 needs to be updated" | `MinimumOSVersion` in `platform/ios/Info.plist` |
| `failed to load MoltenVK` | verify `Payload/Minecraft.app/Frameworks/MoltenVK.framework/MoltenVK` |

See the workflow `Build iOS IPA` log for `file`/`vtool -show-build` output, which
prints the Mach-O platform and deployment target of the produced binary.
