/* PulsePlay helper (PS5 payload) — AGPL-3.0-only.  See util.h. */
#include "util.h"

#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <stdarg.h>
#include <time.h>
#include <unistd.h>
#include <sys/sysctl.h>

#include <ps5/kernel.h>

/* --- PS5 system imports (resolved by the SDK stub libs) --- */
typedef struct notify_request { char pad[45]; char message[3075]; } notify_request_t;
int sceKernelSendNotificationRequest(int, notify_request_t *, size_t, int);

int sceUserServiceInitialize(void *);
int sceUserServiceGetForegroundUser(int *);
int sceRegMgrGetInt(int, int *);
int sceRegMgrSetInt(int, int);
int sceRegMgrGetBin(int, void *, size_t);

extern uint32_t arc4random(void);
extern void arc4random_buf(void *, size_t);

/* Registry entry numbers are a fixed console ABI (documented by idlesauce). */
#define REG_REMOTEPLAY_RP_ENABLE 1098973184

/* Per-user registry entry number: (index-1)*0x10000 + base, for index 1..16. */
static int reg_user_entry(int index, uint32_t base)
{
    if (index < 1 || index > 16) return 0;
    return (int)((uint32_t)(index - 1) * 65536u + base);
}
#define REG_USER_USER_ID(i)    reg_user_entry((i), 125829376u)
#define REG_USER_ACCOUNT_ID(i) reg_user_entry((i), 125830400u)

void note(const char *fmt, ...)
{
    notify_request_t req;
    memset(&req, 0, sizeof(req));
    va_list ap;
    va_start(ap, fmt);
    vsnprintf(req.message, sizeof(req.message), fmt, ap);
    va_end(ap);
    sceKernelSendNotificationRequest(0, &req, sizeof(req), 0);
    printf("%s\n", req.message);
}

static const char B64[] =
    "ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

char *base64(const uint8_t *in, size_t len)
{
    size_t out_len = 4 * ((len + 2) / 3);
    char *out = malloc(out_len + 1);
    if (!out) return NULL;
    size_t i = 0, j = 0;
    while (i < len) {
        uint32_t a = i < len ? in[i++] : 0;
        uint32_t b = i < len ? in[i++] : 0;
        uint32_t c = i < len ? in[i++] : 0;
        uint32_t triple = (a << 16) | (b << 8) | c;
        out[j++] = B64[(triple >> 18) & 0x3F];
        out[j++] = B64[(triple >> 12) & 0x3F];
        out[j++] = B64[(triple >> 6) & 0x3F];
        out[j++] = B64[triple & 0x3F];
    }
    for (size_t k = 0; k < (3 - len % 3) % 3; k++)
        out[out_len - 1 - k] = '=';
    out[out_len] = '\0';
    return out;
}

void rand_hex(char *out, size_t n)
{
    static const char hex[] = "0123456789abcdef";
    for (size_t i = 0; i < n; i++)
        out[i] = hex[arc4random() & 0xF];
    out[n] = '\0';
}

unsigned rand_code6(void) { return arc4random() % 1000000u; }

uint64_t ms_now(void)
{
    struct timespec ts;
    clock_gettime(CLOCK_MONOTONIC, &ts);
    return (uint64_t)ts.tv_sec * 1000 + ts.tv_nsec / 1000000;
}

/* Walk the kinfo_proc table (KERN_PROC_ALL). Field offsets are the FreeBSD
 * amd64 ABI the PS5 kernel uses: ki_structsize @0, ki_pid @72, ki_tdname @447. */
pid_t pid_of(const char *name)
{
    int mib[4] = {CTL_KERN, KERN_PROC, KERN_PROC_ALL, 0};
    size_t sz = 0;
    if (sysctl(mib, 4, NULL, &sz, NULL, 0)) return -1;
    uint8_t *buf = malloc(sz);
    if (!buf) return -1;
    if (sysctl(mib, 4, buf, &sz, NULL, 0)) { free(buf); return -1; }

    pid_t me = getpid(), found = -1;
    for (uint8_t *p = buf; p < buf + sz;) {
        int structsize = *(int *)p;
        if (structsize <= 0) break;
        pid_t pid = *(pid_t *)(p + 72);
        const char *tdname = (const char *)(p + 447);
        if (pid != me && !strcmp(name, tdname)) found = pid;
        p += structsize;
    }
    free(buf);
    return found;
}

uintptr_t remote_sym(pid_t pid, const char *libname, const char *symbol)
{
    uint32_t handle = 0;
    if (kernel_dynlib_handle(pid, libname, &handle) || !handle) return 0;
    return kernel_dynlib_dlsym(pid, handle, symbol);
}

/* Find which registry slot (1..16) holds the current foreground user. */
static int current_user_slot(void)
{
    sceUserServiceInitialize(NULL);
    int user = 0;
    if (sceUserServiceGetForegroundUser(&user)) return -1;
    for (int i = 1; i <= 16; i++) {
        int uid = 0;
        if (sceRegMgrGetInt(REG_USER_USER_ID(i), &uid) == 0 && uid == user)
            return i;
    }
    return -1;
}

int account_id(uint8_t out[8])
{
    int slot = current_user_slot();
    if (slot < 0) return -1;
    return sceRegMgrGetBin(REG_USER_ACCOUNT_ID(slot), out, 8);
}

int enable_remote_play(void)
{
    int val = 0;
    if (sceRegMgrGetInt(REG_REMOTEPLAY_RP_ENABLE, &val)) return -1;
    if (val == 1) return 0;
    if (sceRegMgrSetInt(REG_REMOTEPLAY_RP_ENABLE, 1)) return -1;
    if (sceRegMgrGetInt(REG_REMOTEPLAY_RP_ENABLE, &val)) return -1;
    return val == 1 ? 0 : -1;
}
