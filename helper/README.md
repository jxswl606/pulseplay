# PulsePlay PS5 helper

Our own PS5-side payload — the part that runs on the jailbroken console and that
the PulsePlay Android app talks to over HTTP (TCP 34179). It replaces the Orbit
Play helper so PulsePlay owns the whole protocol end to end.

## What it does (v0)

- **Enables Remote Play without PSN** — sets the one registry flag that gates it
  and generates the pairing PIN by calling into `SceShellUI`, then confirms the
  device registration so chiaki/PulsePlay can pair.
- **Serves PulsePlay's control protocol**: device trust (`/v1/unlock`), review
  proof (`/v1/session`), pairing (`/v1/pair`), and a keep-alive lease. The app
  needs no changes.
- Game list / launch (`/v1/games`, `/v1/games/<id>/launch`) are stubbed for now —
  navigate the PS5 with the on-screen controller. Those land in the next phase.

After pairing, `rp_enable` persists, so streaming keeps working even with the
payload no longer running.

## Build

```
PS5_PAYLOAD_SDK=/opt/ps5-payload-sdk make
```

Produces `pulseplay-helper.elf`. CI builds it on every push (see
`.github/workflows/helper-build.yml`).

## Use

1. Offline-activate your account once (ps5-payload-dev/offact) if you haven't.
2. Inject `pulseplay-helper.elf` with the ELF loader on port 9021 (same as any
   payload). A notification shows a six-digit **trust code**.
3. In PulsePlay, enter your PS5's IP and that trust code, then pair. The 8-digit
   Remote Play PIN is handled automatically — you don't type it.

## Credits

- Offline Remote Play pairing technique documented by **idlesauce**
  (`ps5-remoteplay-get-pin`).
- ptrace call primitive follows the **ps5-payload-dev/websrv** / **HEN-V** lineage.
- Built with **John Törnblom**'s PS5 payload SDK.

Licensed AGPL-3.0-only, like the rest of PulsePlay.
