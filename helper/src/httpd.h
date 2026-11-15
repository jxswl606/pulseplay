/*
 * PulsePlay helper (PS5 payload) — AGPL-3.0-only.
 * A deliberately tiny HTTP/1.1 server: one request per connection, header-only
 * parsing (the control protocol carries everything in the request line + headers).
 */
#pragma once

typedef struct {
    char method[8];
    char path[256];
    char token[80];    /* value after "Authorization: Bearer " */
    char code[16];     /* X-Orbit-Play-Code */
    char review[80];   /* X-Orbit-Play-Review */
} http_req;

/* Open a listening socket on port; returns the fd or -1. */
int httpd_listen(int port);

/* Accept one connection and parse its request; returns the connection fd (to be
 * passed to http_reply) or -1 on a transient error. */
int httpd_next(int listen_fd, http_req *req);

/* Send a JSON response with the given status and close the connection. */
void http_reply(int fd, int status, const char *json);
