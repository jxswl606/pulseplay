#!/usr/bin/env bash
# Host-side protocol test for the PulsePlay helper. Builds the real router.c +
# httpd.c with PS5 pieces mocked, then exercises every /v1 endpoint with curl.
set -u
cd "$(dirname "$0")/.."
PASS=0; FAIL=0
chk() { # chk "label" expected actual
  if [[ "$3" == *"$2"* ]]; then echo "  PASS: $1"; PASS=$((PASS+1));
  else echo "  FAIL: $1 — wanted [$2] got [$3]"; FAIL=$((FAIL+1)); fi
}

echo "== build host test =="
clang -O2 -Wall -Isrc -o /tmp/host_test src/router.c src/httpd.c test/mocks.c test/host_test.c || exit 1

/tmp/host_test >/tmp/ht.out 2>&1 &
SRV=$!; sleep 0.5
CODE=$(grep -o 'TRUST_CODE=[0-9]*' /tmp/ht.out | cut -d= -f2)
echo "trust code = $CODE"
U="http://127.0.0.1:34179"

echo "== tests =="
# unauthenticated endpoint should be rejected
r=$(curl -s -o /dev/null -w '%{http_code}' "$U/v1/session"); chk "no-token session -> 401" "401" "$r"

# wrong trust code
r=$(curl -s -H "X-Orbit-Play-Code: 000000" -X POST "$U/v1/unlock"); chk "bad code -> code err" '"code":"code"' "$r"

# correct trust code -> token
r=$(curl -s -H "X-Orbit-Play-Code: $CODE" -X POST "$U/v1/unlock")
chk "unlock -> token" '"token":"' "$r"
TOK=$(echo "$r" | sed -n 's/.*"token":"\([0-9a-f]*\)".*/\1/p')
echo "  token len = ${#TOK}"; chk "token is 64 hex" "64" "${#TOK}"

AUTH="Authorization: Bearer $TOK"
# proof
r=$(curl -s -H "$AUTH" "$U/v1/session"); chk "session -> proof" '"proof":"' "$r"
PROOF=$(echo "$r" | sed -n 's/.*"proof":"\([0-9a-f]*\)".*/\1/p')

# pair with proof
r=$(curl -s -H "$AUTH" -H "X-Orbit-Play-Review: $PROOF" -X POST "$U/v1/pair"); chk "pair start -> 200 {}" "{}" "$r"
# pair poll (mock: preparing then ready)
r=$(curl -s -H "$AUTH" "$U/v1/pair"); chk "pair poll 1 -> preparing" "preparing" "$r"
r=$(curl -s -H "$AUTH" "$U/v1/pair"); chk "pair poll 2 -> ready+pin+id" '"state":"ready","pin":"12345678","account_id":"AQIDBAUGBwg="' "$r"

# lease
r=$(curl -s -H "$AUTH" -H "X-Orbit-Play-Review: $PROOF" -X POST "$U/v1/session"); chk "lease -> active" '"state":"active"' "$r"
r=$(curl -s -H "$AUTH" "$U/v1/games"); chk "games -> empty list" '"games":[]' "$r"
r=$(curl -s -o /dev/null -w '%{http_code}' -H "$AUTH" -X POST "$U/v1/games/CUSA00001/launch"); chk "launch -> 501" "501" "$r"

# pairing after wrong proof
r=$(curl -s -H "$AUTH" -H "X-Orbit-Play-Review: deadbeef" -X POST "$U/v1/pair"); chk "pair wrong proof -> locked" '"code":"locked"' "$r"

kill $SRV 2>/dev/null
echo "== $PASS passed, $FAIL failed =="
exit $FAIL
