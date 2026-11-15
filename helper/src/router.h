/*
 * PulsePlay helper (PS5 payload) — AGPL-3.0-only.
 * HTTP request routing for the /v1 control protocol, split out so it can be
 * exercised by a host-side test harness (test/host_test.c) as well as the payload.
 */
#pragma once
#include "httpd.h"

/* Initialise request state and return the six-digit device-trust code to show. */
const char *router_init(void);

/* Handle one parsed request and reply on fd (which is then closed). */
void route(int fd, const http_req *r);
