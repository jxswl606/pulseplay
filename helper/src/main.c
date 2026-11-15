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
#include <sys/syscall.h>

#define HELPER_PORT 34179

int main(void)
{
    /* Stop any previous instance first (pid_of matches on thread name, which we
     * only set below), so re-injecting the payload restarts it cleanly. */
    pid_t old;
    while ((old = pid_of("PulsePlayHelper")) > 0) { kill(old, SIGKILL); usleep(100 * 1000); }

    syscall(SYS_thr_set_name, -1, "PulsePlayHelper");

    const char *code = router_init();
    note("PulsePlay helper running\nTrust code: %s", code);

    int lfd = httpd_listen(HELPER_PORT);
    if (lfd < 0) { note("PulsePlay: couldn't open port %d", HELPER_PORT); return 1; }
    printf("PulsePlay helper listening on %d, trust code %s\n", HELPER_PORT, code);

    for (;;) {
        http_req req;
        int fd = httpd_next(lfd, &req);
        if (fd < 0) continue;
        route(fd, &req);
    }
    return 0;
}
