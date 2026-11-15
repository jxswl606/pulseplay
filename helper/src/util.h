/*
 * PulsePlay helper (PS5 payload) — AGPL-3.0-only.
 * Low-level helpers: notifications, base64, randomness, process/registry access.
 *
 * The Remote Play enablement technique (one registry key + the unpatched
 * sceRemoteplayGeneratePinCode / ...ConfirmDeviceRegist path) was first documented
 * by idlesauce (ps5-remoteplay-get-pin); this is an independent reimplementation.
 */
#pragma once

#include <stdint.h>
#include <stddef.h>
#include <sys/types.h>

/* Show an on-screen toast (and echo to stdout for socat injection logs). */
void note(const char *fmt, ...);

/* base64-encode len bytes; returns a malloc'd NUL-terminated string (caller frees). */
char *base64(const uint8_t *in, size_t len);

/* Write n lowercase hex characters of randomness into out (out must hold n + 1). */
void rand_hex(char *out, size_t n);

/* A random six-digit device-trust code, 0..999999. */
unsigned rand_code6(void);

/* Monotonic milliseconds. */
uint64_t ms_now(void);

/* Find a process id by its thread name (e.g. "SceShellUI"); -1 if not found. */
pid_t pid_of(const char *name);

/* Resolve a symbol exported by libname inside the process pid; 0 on failure. */
uintptr_t remote_sym(pid_t pid, const char *libname, const char *symbol);

/* Read the foreground user's 8-byte account id into out; 0 on success. */
int account_id(uint8_t out[8]);

/* Ensure the Remote Play "enabled" registry flag is set; 0 on success. */
int enable_remote_play(void);
