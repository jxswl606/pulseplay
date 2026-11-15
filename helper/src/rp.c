/* PulsePlay helper (PS5 payload) — AGPL-3.0-only.  See rp.h. */
#include "rp.h"
#include "util.h"
#include "pt.h"

#include <string.h>
#include <stdio.h>
#include <stdlib.h>
#include <unistd.h>
#include <pthread.h>
#include <sys/ptrace.h>
#include <sys/syscall.h>
#include <sys/wait.h>
#include <signal.h>

#include <ps5/kernel.h>
#include <ps5/mdbg.h>

#define PAIR_TIMEOUT_MS (120 * 1000)   /* PIN lifetime we enforce ourselves */

/* sceRemoteplayConfirmDeviceRegist status values. */
#define REGIST_STATUS_PAIRED 2
#define REGIST_STATUS_FAILED 3

static pthread_mutex_t g_lock = PTHREAD_MUTEX_INITIALIZER;
static rp_state g_state = RP_IDLE;
static char g_pin[9];       /* "00000000" */
static char g_b64id[16];    /* base64 of 8 bytes = 12 chars */
static volatile bool g_cancel = false;
static bool g_running = false;

static void set_state(rp_state s)
{
    pthread_mutex_lock(&g_lock);
    g_state = s;
    pthread_mutex_unlock(&g_lock);
}

static void fail(const char *msg)
{
    note("PulsePlay: %s", msg);
    set_state(RP_FAILED);
}

static void *pair_thread(void *arg)
{
    (void)arg;
    g_cancel = false;

    if (enable_remote_play()) { fail("couldn't enable Remote Play"); goto done; }

    pid_t shell = pid_of("SceShellUI");
    if (shell <= 0) { fail("SceShellUI not found"); goto done; }

    uintptr_t gen   = remote_sym(shell, "libSceRemoteplay.sprx", "sceRemoteplayGeneratePinCode");
    uintptr_t conf  = remote_sym(shell, "libSceRemoteplay.sprx", "sceRemoteplayConfirmDeviceRegist");
    uintptr_t clrpin = remote_sym(shell, "libSceRemoteplay.sprx", "sceRemoteplayNotifyPinCodeError");
    uintptr_t rcalloc = remote_sym(shell, "libSceLibcInternal.sprx", "calloc");
    uintptr_t rfree   = remote_sym(shell, "libSceLibcInternal.sprx", "free");
    if (!gen || !conf || !clrpin || !rcalloc || !rfree) { fail("missing Remote Play symbols"); goto done; }

    uint8_t id[8];
    if (account_id(id)) { fail("no local account id (offline-activate first)"); goto done; }
    char *b64 = base64(id, sizeof(id));
    if (!b64) { fail("out of memory"); goto done; }

    tracer_t t;
    if (tracer_attach(&t, shell)) { free(b64); fail("couldn't attach to SceShellUI"); goto done; }

    /* Three uint32 scratch buffers in the target: pin, status, errorcode. */
    uintptr_t buf = tracer_call(&t, rcalloc, 3, sizeof(uint32_t), 0, 0, 0, 0);
    if (!buf || buf == (uintptr_t)-1) { tracer_detach(&t); free(b64); fail("alloc failed"); goto done; }
    uintptr_t pin_buf = buf;
    uintptr_t status_buf = buf + sizeof(uint32_t);
    uintptr_t err_buf = buf + 2 * sizeof(uint32_t);

    tracer_call(&t, clrpin, 1, 0, 0, 0, 0, 0);   /* invalidate any stale PIN */

    uintptr_t r = tracer_call(&t, gen, pin_buf, 0, 0, 0, 0, 0);
    uint32_t pin = 0;
    if (r != 0 || mdbg_copyout(shell, pin_buf, &pin, sizeof(pin))) {
        tracer_call(&t, rfree, buf, 0, 0, 0, 0, 0);
        tracer_detach(&t); free(b64);
        fail("couldn't generate PIN"); goto done;
    }

    /* Let SceShellUI run again before we publish the PIN. */
    syscall(SYS_ptrace, PT_CONTINUE, shell, (caddr_t)1, 0);

    pthread_mutex_lock(&g_lock);
    snprintf(g_pin, sizeof(g_pin), "%08u", pin);
    snprintf(g_b64id, sizeof(g_b64id), "%s", b64);
    g_state = RP_READY;
    pthread_mutex_unlock(&g_lock);
    note("PulsePlay pairing ready\nPIN %04u %04u", pin / 10000, pin % 10000);
    free(b64);

    uint64_t start = ms_now();
    bool paired = false;
    while (!g_cancel && ms_now() - start < PAIR_TIMEOUT_MS) {
        /* Stop the UI so we can safely call into it, poll registration, resume. */
        if (kill(shell, SIGSTOP) < 0) break;
        int st = 0;
        if (waitpid(shell, &st, 0) < 0) break;

        int status = 0, err = 0;
        if (tracer_call(&t, conf, status_buf, err_buf, 0, 0, 0, 0) != 0) break;
        mdbg_copyout(shell, status_buf, &status, sizeof(status));
        mdbg_copyout(shell, err_buf, &err, sizeof(err));

        if (status == REGIST_STATUS_PAIRED) { paired = true; break; }
        if (status == REGIST_STATUS_FAILED) break;

        syscall(SYS_ptrace, PT_CONTINUE, shell, (caddr_t)1, 0);
        usleep(250 * 1000);
    }

    tracer_call(&t, clrpin, 1, 0, 0, 0, 0, 0);   /* clear the pending PIN */
    tracer_call(&t, rfree, buf, 0, 0, 0, 0, 0);
    tracer_detach(&t);

    set_state(paired ? RP_PAIRED : RP_FAILED);
    note(paired ? "PulsePlay: device paired" : "PulsePlay: pairing ended");

done:
    pthread_mutex_lock(&g_lock);
    g_running = false;
    pthread_mutex_unlock(&g_lock);
    return NULL;
}

bool rp_begin(void)
{
    pthread_mutex_lock(&g_lock);
    if (g_running) { pthread_mutex_unlock(&g_lock); return false; }
    g_running = true;
    g_state = RP_PREPARING;
    g_pin[0] = g_b64id[0] = '\0';
    pthread_mutex_unlock(&g_lock);

    pthread_t th;
    if (pthread_create(&th, NULL, pair_thread, NULL)) {
        pthread_mutex_lock(&g_lock);
        g_running = false; g_state = RP_FAILED;
        pthread_mutex_unlock(&g_lock);
        return false;
    }
    pthread_detach(th);
    return true;
}

void rp_cancel(void) { g_cancel = true; }

rp_state rp_peek(char *pin8, char *b64id)
{
    pthread_mutex_lock(&g_lock);
    rp_state s = g_state;
    if (pin8) { memcpy(pin8, g_pin, sizeof(g_pin)); }
    if (b64id) { memcpy(b64id, g_b64id, sizeof(g_b64id)); }
    pthread_mutex_unlock(&g_lock);
    return s;
}
