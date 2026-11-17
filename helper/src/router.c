/* PulsePlay helper (PS5 payload) — AGPL-3.0-only.  See router.h. */
#include "router.h"
#include "util.h"
#include "rp.h"

#include <stdio.h>
#include <string.h>

static char g_code[8];     /* six-digit device-trust code, shown on screen */
static char g_token[65];   /* issued on unlock; "" until a device is trusted */
static char g_proof[65];   /* issued on GET /v1/session */

const char *router_init(void)
{
    snprintf(g_code, sizeof(g_code), "%06u", rand_code6());
    g_token[0] = g_proof[0] = '\0';
    return g_code;
}

static int path_is(const http_req *r, const char *m, const char *p)
{
    return !strcmp(r->method, m) && !strcmp(r->path, p);
}

/* True once this device holds the current token. */
static int authed(const http_req *r)
{
    return g_token[0] && !strcmp(r->token, g_token);
}

/* True once any device has unlocked (traded the trust code for a token). The
 * on-screen notifier stops re-posting the code once this is set. */
int router_paired(void) { return g_token[0] != '\0'; }

void route(int fd, const http_req *r)
{
    /* --- device trust: exchange the on-screen 6-digit code for a token --- */
    if (path_is(r, "POST", "/v1/unlock")) {
        if (strcmp(r->code, g_code) != 0) { http_reply(fd, 403, "{\"code\":\"code\"}"); return; }
        rand_hex(g_token, 64);
        char body[96];
        snprintf(body, sizeof(body), "{\"token\":\"%s\"}", g_token);
        http_reply(fd, 200, body);
        return;
    }

    /* everything below requires the token */
    if (!authed(r)) { http_reply(fd, 401, "{\"code\":\"locked\"}"); return; }

    /* --- one-time review proof (used by pair + session) --- */
    if (path_is(r, "GET", "/v1/session")) {
        rand_hex(g_proof, 64);
        char body[96];
        snprintf(body, sizeof(body), "{\"proof\":\"%s\"}", g_proof);
        http_reply(fd, 200, body);
        return;
    }

    /* --- pairing --- */
    if (path_is(r, "POST", "/v1/pair")) {
        if (!g_proof[0] || strcmp(r->review, g_proof) != 0) { http_reply(fd, 403, "{\"code\":\"locked\"}"); return; }
        if (!rp_begin()) { http_reply(fd, 409, "{\"code\":\"busy\"}"); return; }
        http_reply(fd, 200, "{}");
        return;
    }
    if (path_is(r, "GET", "/v1/pair")) {
        char pin[9], b64[16];
        rp_state s = rp_peek(pin, b64);
        if (s == RP_PREPARING) { http_reply(fd, 200, "{\"state\":\"preparing\"}"); return; }
        if (s == RP_READY || s == RP_PAIRED) {
            char body[160];
            snprintf(body, sizeof(body),
                     "{\"state\":\"ready\",\"pin\":\"%s\",\"account_id\":\"%s\"}", pin, b64);
            http_reply(fd, 200, body);
            return;
        }
        http_reply(fd, 200, "{\"state\":\"expired\"}");
        return;
    }
    if (path_is(r, "POST", "/v1/cancel")) { rp_cancel(); http_reply(fd, 200, "{}"); return; }

    /* --- playback lease: a no-op for us (rp_enable persists), but the app
     *     opens/renews it, so answer "active". --- */
    if (path_is(r, "POST", "/v1/session"))      { http_reply(fd, 200, "{\"state\":\"active\"}"); return; }
    if (path_is(r, "POST", "/v1/session/end"))  { http_reply(fd, 200, "{}"); return; }

    /* --- games: not implemented in v0 (navigate the PS5 yourself) --- */
    if (path_is(r, "GET", "/v1/games")) { http_reply(fd, 200, "{\"games\":[]}"); return; }
    if (!strcmp(r->method, "POST") && !strncmp(r->path, "/v1/games/", 10)) {
        http_reply(fd, 501, "{\"code\":\"native_error\"}");
        return;
    }

    /* --- away-from-home link: not supported by our helper (use Tailscale) --- */
    if (!strncmp(r->path, "/v1/link", 8)) { http_reply(fd, 400, "{\"code\":\"native_error\"}"); return; }

    http_reply(fd, 404, "{\"code\":\"protocol\"}");
}
