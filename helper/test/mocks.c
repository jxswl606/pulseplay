/*
 * Host-test mocks — AGPL-3.0-only. Stand-ins for the PS5-only pieces so the real
 * router.c + httpd.c can run and be exercised on a normal Linux box. NOT built
 * into the payload.
 */
#include "util.h"
#include "rp.h"
#include <stdio.h>
#include <string.h>
#include <stdarg.h>

/* --- util mocks --- */
void note(const char *fmt, ...)
{
    va_list ap; va_start(ap, fmt);
    vprintf(fmt, ap); va_end(ap); printf("\n");
}

/* Deterministic-ish hex so successive tokens/proofs differ (good enough to test). */
void rand_hex(char *out, size_t n)
{
    static unsigned long s = 0x12345;
    static const char hex[] = "0123456789abcdef";
    for (size_t i = 0; i < n; i++) { s = s * 6364136223846793005UL + 1; out[i] = hex[(s >> 33) & 0xF]; }
    out[n] = '\0';
}

unsigned rand_code6(void) { return 123456; }   /* fixed so the test script knows it */

/* unused by the router, but referenced by util.h */
uint64_t ms_now(void) { return 0; }
pid_t pid_of(const char *n) { (void)n; return -1; }
uintptr_t remote_sym(pid_t p, const char *l, const char *s) { (void)p;(void)l;(void)s; return 0; }
int account_id(uint8_t o[8]) { (void)o; return -1; }
int enable_remote_play(void) { return 0; }

/* --- rp mocks: pretend pairing becomes READY after a couple of polls --- */
static rp_state st = RP_IDLE;
static int polls;
bool rp_begin(void) { st = RP_PREPARING; polls = 0; return true; }
void rp_cancel(void) { st = RP_IDLE; }
rp_state rp_peek(char *pin, char *b64)
{
    if (st == RP_PREPARING && ++polls >= 2) st = RP_READY;
    if (pin) strcpy(pin, st >= RP_READY ? "12345678" : "");
    if (b64) strcpy(b64, st >= RP_READY ? "AQIDBAUGBwg=" : "");   /* base64 of 8 bytes */
    return st;
}
