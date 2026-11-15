/* Host-test server — AGPL-3.0-only. Runs the real router + HTTP server on Linux. */
#include "httpd.h"
#include "router.h"
#include <stdio.h>

int main(void)
{
    const char *code = router_init();
    printf("TRUST_CODE=%s\n", code);
    fflush(stdout);

    int lfd = httpd_listen(34179);
    if (lfd < 0) { perror("listen"); return 1; }
    for (;;) {
        http_req req;
        int fd = httpd_next(lfd, &req);
        if (fd < 0) continue;
        route(fd, &req);
    }
    return 0;
}
