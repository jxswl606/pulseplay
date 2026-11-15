/* PulsePlay helper (PS5 payload) — AGPL-3.0-only.  See pt.h. */
#include "pt.h"

#include <string.h>
#include <stdio.h>
#include <unistd.h>
#include <errno.h>
#include <sys/ptrace.h>
#include <sys/syscall.h>
#include <sys/wait.h>
#include <machine/reg.h>

#include <ps5/kernel.h>
#include <ps5/mdbg.h>

/* libkernel's dynlib handle is a fixed well-known value; its image base starts
 * with an INT3 we use as the return trap after a forced call. */
#define LIBKERNEL_HANDLE 0x2001

/* An authid + capability set with the privileges ptrace on a system process needs. */
#define DEBUG_AUTHID 0x4800000000010003ULL

int tracer_attach(tracer_t *t, pid_t pid)
{
    memset(t, 0, sizeof(*t));
    pid_t me = getpid();
    if (me == pid) return -1;

    uint8_t full_caps[16];
    memset(full_caps, 0xFF, sizeof(full_caps));

    uint64_t og_authid = kernel_get_ucred_authid(me);
    if (!og_authid) return -1;
    uint8_t og_caps[16];
    if (kernel_get_ucred_caps(me, og_caps)) return -1;

    if (kernel_set_ucred_authid(me, DEBUG_AUTHID)) return -1;
    if (kernel_set_ucred_caps(me, full_caps)) {
        kernel_set_ucred_authid(me, og_authid);
        return -1;
    }

    if ((int)syscall(SYS_ptrace, PT_ATTACH, pid, 0, 0) < 0) {
        kernel_set_ucred_authid(me, og_authid);
        kernel_set_ucred_caps(me, og_caps);
        return -1;
    }
    int status = 0;
    if (waitpid(pid, &status, 0) < 0) return -1;

    t->pid = pid;
    t->saved_authid = og_authid;
    memcpy(t->saved_caps, og_caps, sizeof(og_caps));
    return 0;
}

void tracer_detach(tracer_t *t)
{
    if (!t->pid) return;
    syscall(SYS_ptrace, PT_DETACH, t->pid, 0, 0);
    kernel_set_ucred_authid(getpid(), t->saved_authid);
    kernel_set_ucred_caps(getpid(), t->saved_caps);
    t->pid = 0;
}

uintptr_t tracer_call(tracer_t *t, uintptr_t addr,
                      uintptr_t a, uintptr_t b, uintptr_t c,
                      uintptr_t d, uintptr_t e, uintptr_t f)
{
    if (!addr) { errno = EINVAL; return (uintptr_t)-1; }

    struct reg saved, call;
    if ((int)syscall(SYS_ptrace, PT_GETREGS, t->pid, (caddr_t)&saved, 0) < 0)
        return (uintptr_t)-1;
    call = saved;

    call.r_rip = (register_t)addr;
    call.r_rdi = (register_t)a;
    call.r_rsi = (register_t)b;
    call.r_rdx = (register_t)c;
    call.r_rcx = (register_t)d;
    call.r_r8  = (register_t)e;
    call.r_r9  = (register_t)f;

    if (!t->libkernel_base) {
        t->libkernel_base = kernel_dynlib_mapbase_addr(t->pid, LIBKERNEL_HANDLE);
        if (!t->libkernel_base) return (uintptr_t)-1;
    }

    /* Push a return address that lands on libkernel's INT3 so the call traps. */
    call.r_rsp = (register_t)(call.r_rsp - sizeof(uintptr_t));
    if ((int)syscall(SYS_ptrace, PT_SETREGS, t->pid, (caddr_t)&call, 0) < 0)
        return (uintptr_t)-1;
    mdbg_copyin(t->pid, &t->libkernel_base, call.r_rsp, sizeof(t->libkernel_base));

    if ((int)syscall(SYS_ptrace, PT_CONTINUE, t->pid, (caddr_t)1, 0) < 0)
        return (uintptr_t)-1;

    int status = 0;
    if (waitpid(t->pid, &status, 0) < 0) return (uintptr_t)-1;
    if (!WIFSTOPPED(status) || WSTOPSIG(status) != SIGTRAP) return (uintptr_t)-1;

    if ((int)syscall(SYS_ptrace, PT_GETREGS, t->pid, (caddr_t)&call, 0) < 0)
        return (uintptr_t)-1;
    /* Restore the thread exactly as we found it. */
    if ((int)syscall(SYS_ptrace, PT_SETREGS, t->pid, (caddr_t)&saved, 0) < 0)
        return (uintptr_t)-1;

    return (uintptr_t)call.r_rax;
}
