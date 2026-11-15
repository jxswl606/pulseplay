/* PulsePlay helper (PS5 payload) — AGPL-3.0-only.  See httpd.h. */
#include "httpd.h"

#include <stdio.h>
#include <string.h>
#include <strings.h>
#include <stdlib.h>
#include <unistd.h>
#include <sys/socket.h>
#include <netinet/in.h>

int httpd_listen(int port)
{
    int fd = socket(AF_INET, SOCK_STREAM, 0);
    if (fd < 0) return -1;
    int one = 1;
    setsockopt(fd, SOL_SOCKET, SO_REUSEADDR, &one, sizeof(one));

    struct sockaddr_in addr;
    memset(&addr, 0, sizeof(addr));
    addr.sin_family = AF_INET;
    addr.sin_port = htons((unsigned short)port);
    addr.sin_addr.s_addr = htonl(INADDR_ANY);
    if (bind(fd, (struct sockaddr *)&addr, sizeof(addr)) < 0) { close(fd); return -1; }
    if (listen(fd, 8) < 0) { close(fd); return -1; }
    return fd;
}

/* Copy the value of header `name` from the raw header block into out. */
static void header(const char *headers, const char *name, char *out, size_t out_sz)
{
    out[0] = '\0';
    size_t nlen = strlen(name);
    for (const char *p = headers; p && *p;) {
        const char *eol = strstr(p, "\r\n");
        size_t line_len = eol ? (size_t)(eol - p) : strlen(p);
        if (line_len > nlen && !strncasecmp(p, name, nlen) && p[nlen] == ':') {
            const char *v = p + nlen + 1;
            while (*v == ' ' || *v == '\t') v++;
            size_t vlen = (eol ? (size_t)(eol - v) : strlen(v));
            if (vlen >= out_sz) vlen = out_sz - 1;
            memcpy(out, v, vlen);
            out[vlen] = '\0';
            return;
        }
        if (!eol) break;
        p = eol + 2;
    }
}

int httpd_next(int listen_fd, http_req *req)
{
    int fd = accept(listen_fd, NULL, NULL);
    if (fd < 0) return -1;

    /* Read until the end of the header block (or the buffer fills). */
    char buf[2048];
    size_t len = 0;
    while (len < sizeof(buf) - 1) {
        ssize_t n = recv(fd, buf + len, sizeof(buf) - 1 - len, 0);
        if (n <= 0) break;
        len += (size_t)n;
        buf[len] = '\0';
        if (strstr(buf, "\r\n\r\n")) break;
    }
    buf[len] = '\0';

    memset(req, 0, sizeof(*req));
    if (sscanf(buf, "%7s %255s", req->method, req->path) != 2) {
        http_reply(fd, 400, "{\"code\":\"protocol\"}");
        return -1;
    }

    const char *headers = strstr(buf, "\r\n");
    if (headers) {
        headers += 2;
        char auth[96];
        header(headers, "Authorization", auth, sizeof(auth));
        if (!strncasecmp(auth, "Bearer ", 7))
            snprintf(req->token, sizeof(req->token), "%s", auth + 7);
        header(headers, "X-Orbit-Play-Code", req->code, sizeof(req->code));
        header(headers, "X-Orbit-Play-Review", req->review, sizeof(req->review));
    }
    return fd;
}

void http_reply(int fd, int status, const char *json)
{
    const char *reason = status < 300 ? "OK" : status < 500 ? "Client Error" : "Server Error";
    char head[256];
    int body_len = (int)strlen(json);
    int n = snprintf(head, sizeof(head),
                     "HTTP/1.1 %d %s\r\n"
                     "Content-Type: application/json\r\n"
                     "Content-Length: %d\r\n"
                     "Connection: close\r\n\r\n",
                     status, reason, body_len);
    send(fd, head, (size_t)n, 0);
    send(fd, json, (size_t)body_len, 0);
    close(fd);
}
