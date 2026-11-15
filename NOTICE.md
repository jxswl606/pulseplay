# Credits and third-party notices

PulsePlay is free, open-source software licensed under the **GNU Affero General
Public License v3.0** (see `LICENSE`). It is **not for sale**. It streams from
jailbroken PS5 consoles without PSN, and stands on the work of others.

## Remote Play engine
- **chiaki-ng** by **Street Pea (streetpea)** — the maintained Remote Play engine
  and Android client (MediaCodec decode, controller handling) PulsePlay builds on.
  https://github.com/streetpea/chiaki-ng  (AGPL-3.0)
- **Chiaki** by **Florian Märkl (thestr4ng3r)** — the original open-source PS4/PS5
  Remote Play client that everything above descends from.
  https://github.com/thestr4ng3r/chiaki  (AGPL-3.0 / LGPL components)

## PS5 helper (`helper/`)
PulsePlay includes its **own** PS5-side helper payload (`helper/`), which enables
Remote Play pairing without PSN and serves the app's control protocol. It is an
independent reimplementation that stands on:
- **Orbit Play** by **saawant12** — the original PS5 helper and the control protocol
  PulsePlay's helper reimplements and the app speaks.
  https://github.com/saawant12/orbit-play-ps5
- **ps5-remoteplay-get-pin** by **idlesauce** — the offline Remote Play pairing
  technique (enabling the feature and generating the pairing PIN via SceShellUI)
  that PulsePlay's helper reimplements clean-room.
  https://github.com/idlesauce/ps5-remoteplay-get-pin
- **PS5 Payload SDK** by **John Törnblom / ps5-payload-dev** — the toolchain the
  helper is built with.
  https://github.com/ps5-payload-dev/sdk
- **ps5-payload-dev/websrv** and **astrelsky/HEN-V** — the ptrace call-injection
  technique the helper's tracer follows.

## Away-from-home (Orbit Link / Tailscale)
- `engine/orbit-link/` vendors the client node of **Orbit Link** by **saawant12**
  (https://github.com/saawant12/orbit-link), **GPL-3.0-or-later / AGPL** (see
  `engine/orbit-link/LICENSE-AGPL`), built unmodified and bundled.
- **boringtun** by **Cloudflare, Inc.** — BSD-3-Clause — the WireGuard implementation
  used by the Orbit Link node. Copyright (c) 2019 Cloudflare, Inc.
- **Tailscale** (https://tailscale.com) — recommended (not bundled) for reaching your
  PS5 over the internet; PulsePlay needs no VPS.

## Community
The **PS5/PS4 homebrew and jailbreak community** — payload loaders, the jailbreak,
and the tooling that makes any of this possible.

## License obligations
As an AGPL-3.0 network application, PulsePlay must offer its complete corresponding
source to anyone who interacts with it over a network. The source lives in this
repository. All upstream LICENSE texts and native-dependency notices are retained
(under `third_party/` as the engine integrates, and `engine/orbit-link/` for the
away node).

## Trademarks
"PlayStation", "PS5", "PS4" and "DualSense" are trademarks of Sony Interactive
Entertainment Inc. PulsePlay is an independent project and is **not affiliated with,
authorized, or endorsed by Sony**.
