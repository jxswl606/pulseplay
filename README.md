# PulsePlay

Free, open-source **Remote Play for jailbroken PS5 consoles — without PSN**.
Android first, then iOS (sideload), then Switch homebrew. Not for sale.

> Status: **M1 in progress** — project skeleton that builds to an installable
> Android APK. Streaming is not wired up yet.

## How it works (short version)

Your jailbroken PS5 runs the **Orbit Play helper** (installed via Payload
Manager). PulsePlay pairs with it using a **6-digit trust code** shown on the
PS5, which hands the app a device token. The helper then performs PlayStation's
own Remote Play registration natively and returns an 8-digit PIN; the app turns
that into a saved stream key. After that, one tap streams your PS5 — no PSN
account anywhere in the loop.

See `docs/ARCHITECTURE-NOTES.md` for the source-verified protocol details.

## Build

Requires nothing locally — GitHub Actions builds the APK on every push
(see `.github/workflows/android-build.yml`). Download it from the workflow run's
**Artifacts**, then install the `.apk` on your phone (enable "install unknown
apps").

To build locally instead: JDK 17 + Android SDK, then `./gradlew assembleDebug`.

## Credits & license

Built on Orbit Play (saawant12), chiaki-ng (streetpea) and Chiaki
(thestr4ng3r), plus the PS5 homebrew community. Licensed **AGPL-3.0-only**.
See `NOTICE.md` and `LICENSE`.
