/*
 * PulsePlay helper (PS5 payload) — AGPL-3.0-only.
 * Minimal ptrace "call a function in another process" tracer. The technique
 * (elevate ucred, PT_ATTACH, point RIP at the target with a libkernel INT3
 * return trap, catch SIGTRAP, read RAX) is standard PS5 homebrew practice
 * (see ps5-payload-dev/websrv, astrelsky/HEN-V).
 */
#pragma once
#include <stdint.h>
#include <sys/types.h>

typedef struct {
    pid_t pid;
    uint64_t saved_authid;
    uint8_t saved_caps[16];
    uintptr_t libkernel_base;
} tracer_t;

/* Attach to pid (elevating our credentials first). 0 on success. */
int tracer_attach(tracer_t *t, pid_t pid);

/* Detach and restore our original credentials. */
void tracer_detach(tracer_t *t);

/* Call addr(a,b,c,d,e,f) in the traced process; returns its RAX, or -1 on error. */
uintptr_t tracer_call(tracer_t *t, uintptr_t addr,
                      uintptr_t a, uintptr_t b, uintptr_t c,
                      uintptr_t d, uintptr_t e, uintptr_t f);
