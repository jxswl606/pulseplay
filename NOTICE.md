# Credits and third-party notices

PulsePlay is free, open-source software licensed under the **GNU Affero General
Public License v3.0** (see `LICENSE`). It is **not for sale**. It streams from
jailbroken PS5 consoles without PSN, and stands on the work of others:

- **Orbit Play** by **saawant12** — the PS5-side helper, the trust-code pairing
  design, and the Orbit Link away-from-home node. PulsePlay reuses Orbit Play's
  PS5 helper unchanged and speaks its helper protocol.
  https://github.com/saawant12/orbit-play-ps5  (GPL-3.0+ / AGPL-3.0-only components)
- **chiaki-ng** by **streetpea** — the maintained Remote Play engine and Android
  app (touch controls, MediaCodec decode) that PulsePlay's Android client builds on.
  https://github.com/streetpea/chiaki-ng  (AGPL-3.0)
- **Chiaki** by **Florian Märkl (thestr4ng3r)** — the original open-source PS4/PS5
  Remote Play client that everything above descends from.
  https://github.com/thestr4ng3r/chiaki  (AGPL-3.0 / LGPL components)
- **The PS5 homebrew community** — payload loaders, the jailbreak, and the
  tooling that makes any of this possible.

As an AGPL-3.0 network application, PulsePlay must offer its complete
corresponding source to its users. The source lives in this repository.

Full upstream license texts and native-dependency notices are retained under
`third_party/` as the engine is integrated.
