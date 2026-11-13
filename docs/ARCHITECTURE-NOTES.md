# PulsePlay — verified architecture notes

_Source-verified 2026-11-13 from Orbit Play v0.8.0 source + chiaki-ng. Facts below are
read directly from code, not assumed._

## Foundation verification (all confirmed real)
- `saawant12/orbit-play-ps5` exists; `payloads.json` live; v0.8.0 source tarball is a real
  9.1 MB download (sha256 9191e8047389f30f6de7b6d7f46e09b510aa7eb132841228d53e25364215bc7d).
- Orbit source layout: `app/` (Python/Qt desktop), `ps5/` (helper ELF), `link/` (Rust Orbit
  Link node), `docs/`, `tools/`, `i18n/`, `tests/`.
- Key insight: Orbit's desktop app uses **chiaki-py (Python bindings) + a patch**
  (`app/patches/chiaki-py-0.3.3-orbitplay.patch`), NOT the chiaki-ng Android app.

## Client ↔ helper protocol (from app/orbit_play/helper.py)
- Helper control port: **TCP 34179**, plain HTTP, numeric private-LAN IPv4 only
  (10/8, 172.16/12, 192.168/16 enforced client-side). No DNS, no proxies.
- Away-from-home: same requests tunnel through Orbit Link's forward on 127.0.0.1 (`via`).
- Common headers: `X-Orbit-Play-Client: 1`, `Connection: close`,
  `Authorization: Bearer <64-hex device token>`, `X-Orbit-Play-Code: <6 digits>`,
  `X-Orbit-Play-Review: <64-hex proof>`.

### Trust-code pairing (this is M2) — `POST /v1/unlock`
- Send `X-Orbit-Play-Code: <6-digit>` (shown in a PS5 notification).
- Success → `{ "token": "<64 hex>" }` = the device token, reused as Bearer afterwards.
- Errors: `locked` (code needed), `code` (wrong), `rate_limit` (too many attempts).

### Orbit Link routes (this is M3) — `/v1/link/{status,setup,invite,leave}`
- Header fields: `X-Orbit-Play-Link-Server`, `-Network`, `-Code`.

### Helper error vocabulary (drives all client UX messaging)
setup_unavailable, review_required, snapshot_failed, configuration_unsupported,
backup_failed/invalid, no_backup, already_prepared, recovery_required, restore_conflict,
locked, code, rate_limit, inactive, activation_required, busy, user_changed, native_error,
expired, remote_play_disabled, session_expired, live_failed/recovery, link_home, link_key,
link_busy, link_unavailable, route, streaming, game, game_failed, power_failed.

## What this means for PulsePlay M1 (refines the handoff)
The handoff said "register via getpin/helper PIN". More precisely:
- The **helper performs the PSN-equivalent Remote Play registration natively on the PS5**
  (note `native_error`, `activation_required`, `remote_play_disabled` codes — the PS5-side
  native Remote Play API is what the helper drives).
- The **client never does PSN registration**. Its job is only:
  1. Obtain a 64-hex device token via `POST /v1/unlock` + 6-digit code (trust-code flow).
  2. Open a chiaki Remote Play stream to the console (standard chiaki ports; 9295 is the
     Remote Play port the Orbit Link node forwards to 127.0.0.1 away-from-home).
- So "strip PSN registration from the chiaki-ng fork" is correct — the helper replaces it.
  M1 = device-token unlock + chiaki LAN stream. M2 = polish that pairing UX.

## COMPLETE M1 flow — RESOLVED (from service.py lines 1152-1180, engine.py)

Full helper endpoint map (app/orbit_play, GET/POST on port 34179):
  /v1/health /v1/status /v1/unlock /v1/pair /v1/session /v1/session/end
  /v1/trust/key /v1/trust/forget /v1/games /v1/power/off /v1/quit /v1/cancel
  /v1/log /v1/diagnostics /v1/link/{status,setup,invite,leave}

The pairing + first-stream sequence the Android client must reproduce:
1. TRUST (M2):  POST /v1/unlock  + header X-Orbit-Play-Code: <6-digit from PS5 notif>
   -> { "token": "<64 hex>" }. Token sent as `Authorization: Bearer` thereafter.
2. BEGIN PAIR: POST /v1/pair  (with X-Orbit-Play-Review proof) -> helper asks the PS5 to
   start a native Remote Play registration request.
3. GET PIN:    GET /v1/pair -> { "account_id": <...>, "pin": "<8 digits>" }.
   THIS is the handoff's "getpin": the 8-digit RP registration PIN + the console's local
   account id, both obtained natively by the helper (no PSN).
4. REGISTER (client-side, chiaki's own regist): chiaki_regist with host + 8-digit pin +
   account_id  ->  registration = { regist_key, morning (16-byte RP key, hex), host,
   target:"PS5_1" }. Standard chiaki RegisteredHost material.
5. STREAM: open a chiaki Session(registration) to the console. Helper brackets the live
   session with /v1/session ... /v1/session/end. Away-from-home, the same traffic rides
   Orbit Link's 127.0.0.1 forward (Remote Play port 9295).

=> For ANDROID we do NOT reuse Orbit's chiaki-py (Python/desktop). We take chiaki-ng's
   native C lib + its existing Android JNI/MediaCodec/touch layer, strip its PSN-login
   registration UI, and drive chiaki regist with the pin+account_id the helper hands us.
   Registration data (regist_key + morning) is exactly what chiaki already produces/stores.

Orbit's chiaki-py patch (NATIVE_ENGINE.md) is desktop-only, but tells us which stream
features matter: full touch positions, rumble, adaptive-trigger effects, DualSense
intensity, haptics, microphone. chiaki-ng's Android app already handles the video/touch.

License: the engine patch and combined source are AGPL-3.0-only. Keep upstream licenses,
patched-source revisions and third-party notices with any distribution.
