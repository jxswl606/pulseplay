/*
 * PulsePlay helper (PS5 payload) — AGPL-3.0-only.
 * Our own PS5-side helper: serves PulsePlay's control protocol on TCP 34179 and
 * performs offline Remote Play pairing. v0 scope: device trust + pairing +
 * keep-alive. Game list / launch are stubbed until the next phase.
 *
 * Credits: Remote Play offline-pairing technique documented by idlesauce; ptrace
 * call primitive from the ps5-payload-dev/websrv lineage; SDK by John Törnblom.
 */
#include "httpd.h"
#include "router.h"
#include "util.h"

#include <stdio.h>
#include <unistd.h>
#include <signal.h>
#include <pthread.h>
#include <sys/syscall.h>

#define HELPER_PORT 34179

static const char *g_code_disp;

/* PS5 notifications vanish after ~5s, so re-post the trust code every few
 * seconds — keeping it on screen until a device pairs, no need to rush. Stops
 * the moment pairing happens, and caps at ~2 min so it never spams forever. */
static void *code_notifier(void *arg)
{
    (void)arg;
    for (int i = 0; i < 30 && !router_paired(); i++) {
        note("PulsePlay — enter this code in the app to pair\nTrust code: %s", g_code_disp);
        sleep(4);
    }
    return NULL;
}

int main(void)
{
    /* Stop any previous instance first (pid_of matches on thread name, which we
     * only set below), so re-injecting the payload restarts it cleanly. */
    pid_t old;
    while ((old = pid_of("PulsePlayHelper")) > 0) { kill(old, SIGKILL); usleep(100 * 1000); }

    syscall(SYS_thr_set_name, -1, "PulsePlayHelper");

    const char *code = router_init();

    int lfd = httpd_listen(HELPER_PORT);
    if (lfd < 0) { note("PulsePlay: couldn't open port %d\n(another helper may still be running — reboot and load only this one)", HELPER_PORT); return 1; }
    printf("PulsePlay helper listening on %d, trust code %s\n", HELPER_PORT, code);

    /* Keep the trust code on screen until a device pairs. */
    g_code_disp = code;
    pthread_t th;
    if (pthread_create(&th, NULL, code_notifier, NULL) == 0) pthread_detach(th);
    else note("PulsePlay — Trust code: %s", code);   /* fallback: show once */

    for (;;) {
        http_req req;
        int fd = httpd_next(lfd, &req);
        if (fd < 0) continue;
        route(fd, &req);
    }
    return 0;
}
