/*
 * PulsePlay helper (PS5 payload) — AGPL-3.0-only.
 * Remote Play offline pairing: enable the feature, generate a pairing PIN by
 * calling into SceShellUI, and keep confirming device registration until a
 * client (chiaki) completes the handshake.
 */
#pragma once
#include <stdbool.h>

typedef enum {
    RP_IDLE,       /* nothing in progress */
    RP_PREPARING,  /* generating the PIN */
    RP_READY,      /* PIN + account id available, waiting for the device */
    RP_PAIRED,     /* a device completed registration */
    RP_FAILED,     /* generation or pairing failed */
} rp_state;

/* Begin a pairing attempt in a background thread. Returns false if one is
 * already running. */
bool rp_begin(void);

/* Ask the running attempt to stop. */
void rp_cancel(void);

/* Copy the current state out. pin8 (>=9 bytes) gets the 8-digit PIN string and
 * b64id (>=16 bytes) the base64 account id, valid only when state is RP_READY
 * or later. */
rp_state rp_peek(char *pin8, char *b64id);
