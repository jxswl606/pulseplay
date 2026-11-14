# Orbit Link

**The network behind Orbit Play's play away from home.** No account, no router
setup, no VPN app.

Orbit Link connects your PS5 and your computers **directly to each other**,
encrypted end to end, even when both are behind home routers. An Orbit Link
server only introduces them. In Orbit Play:

- the **PS5 side** runs inside the Orbit Play helper (`crates/link-ps5`, linked
  into `orbit-play.elf`; see [the helper](../ps5/README.md#play-away-from-home)),
- the **computer side** is `orbit-link-node` (`crates/link-node`), which the
  Orbit Play app bundles and runs itself (see [the app](../app/README.md#play-away-from-home)),
- the **server** (`crates/link-server`) is Orbit Play's at `link.orbitplay.dev`,
  or your own.

```
                 Orbit Link server
              (introduces the devices)
                ▲                  ▲
    finds peers │                  │ finds peers
                │                  │
  Orbit Play app ◀════════════════▶ PS5 with the Orbit Play helper
 127.0.0.1:9295 …    direct, encrypted     Remote Play, helper
```

## How it works

1. Each device keeps an encrypted session with its Orbit Link server. The
   server tells it which other devices are in its network and where they might
   be reached.
2. Both devices send small probes to each other at the same time. This opens
   both home routers for the other device ("hole punching"), so no port
   forwarding is needed. Devices in the same home connect over the local
   network.
3. Traffic flows directly between the devices, encrypted end to end with the
   WireGuard protocol. The server never sees it.
4. Strict routers, as on many phone hotspots and mobile networks, give each
   connection a different port. Then the devices try many ports at once for a
   few seconds until one gets through. If the routers allow it, the devices also
   ask their router to forward their port (UPnP or NAT-PMP).
5. If no direct connection is possible, a server **with relay** passes the
   encrypted traffic along. Orbit Play's server does not relay.

More detail: [How it works](docs/how-it-works.md).

## Using it

Everything happens in Orbit Play: at home, **Console setup → Play away from
home → Turn on**. The PS5 joins a network on Orbit Play's server (or yours, under
**Use your own Orbit Link server**), adds the computer and forwards the helper
and Remote Play ports (TCP 34179 and 9295, UDP 9295, 9296, 9297 and 9302) to the
computer's `127.0.0.1`. Away from home, **Start playing** as usual. A computer that
was never home uses **Not at home? Set up from anywhere** with the away code the PS5
shows under **Play away from home → Get a code** on its Orbit Play page; the code
appears only on the PS5's own screen. **Turn off** at home removes the PS5 and the
computer; away from home it removes only this computer, as does **Forget saved
connections**.

`orbit-link-node` also runs on its own, for another computer or for other
services on the console: `orbit-link-node --data DIR` serves its page on
`http://127.0.0.1:34180`, where it joins with a code and forwards any port of a
device in the network. The PS5's payload loader (TCP 9021) stays blocked for other
devices.

## Orbit Play's server or your own

| | Orbit Play's server | Your own server |
| --- | --- | --- |
| Setup | None | A machine with Docker, one open UDP port and the dashboard's TCP port (devices join over it) |
| New networks | Orbit Play creates one for you | Codes from your dashboard (or open, like the public one) |
| When a direct connection is impossible | The devices cannot connect | The server relays (optional) |
| What the server knows | Device names, their public and local network addresses, how they're connected | The same, on your machine |

Traffic between devices is encrypted end to end either way. To switch servers,
turn play away from home off at home and on again with the other server.

## Run your own server

You need a machine with Docker that your devices can reach: a small VPS, or a
home server with one UDP port forwarded on the router. Take the `link/` folder
from the source archive attached to each
[Orbit Play release](https://github.com/saawant12/orbit-play-ps5/releases), then:

```sh
cd link
./install.sh
```

The installer asks for the server's public address, an optional domain for an
HTTPS dashboard, whether to relay and whether anyone may create a network. It
writes `.env`, builds the server (a few minutes the first time) and prints the
dashboard address and admin password.

Open in the firewall:

| Port | Protocol | Purpose |
| --- | --- | --- |
| 41700 | **UDP** | Devices connect here |
| 8080 | TCP | Dashboard and joining, without HTTPS |
| 80, 443 | TCP | Dashboard and joining with the HTTPS profile (automatic certificates) |

Then in the dashboard choose **New network** and **Add a device**. In Orbit Play,
enter the server's address and that code under **Use your own Orbit Link
server**, then **Turn on**: the PS5 joins with the code and adds the computer.

To run it by hand, `cp .env.example .env`, edit `.env`, then
`docker compose up -d --build` (with a domain:
`docker compose --profile https up -d --build`).

### Settings

Set these in `.env`; `install.sh` writes it for you.

| Variable | Default | Meaning |
| --- | --- | --- |
| `ORBIT_LINK_PUBLIC_HOST` | (empty) | Domain or IP devices connect to over UDP. If empty, set it in the dashboard |
| `ORBIT_LINK_UDP_PORT` | `41700` | UDP port the server listens on |
| `ORBIT_LINK_PUBLIC_UDP_PORT` | `ORBIT_LINK_UDP_PORT` | UDP port devices are told to use, when the published port differs (a port mapping) |
| `ORBIT_LINK_ADMIN_PASSWORD` | (generated) | Dashboard password, at least 10 characters |
| `ORBIT_LINK_RELAY` | `on` | Relay traffic when devices cannot connect directly |
| `ORBIT_LINK_RELAY_MBPS` | `0` | Relay limit per device in Mbit/s; 0 for none |
| `ORBIT_LINK_OPEN_NETWORKS` | `off` | `on` lets any device create a network (a public server) |
| `ORBIT_LINK_MAX_DEVICES` | `50` | Devices per network |
| `ORBIT_LINK_MAX_NETWORKS` | `10000` | Networks on the server |
| `ORBIT_LINK_PRUNE_DAYS` | `0` | Delete networks unused for this many days; 0 keeps them |
| `ORBIT_LINK_PRUNE_UNUSED_HOURS` | `0` | Delete networks no device ever connected to after this many hours; 0 keeps them |
| `ORBIT_LINK_NAME` | `Orbit Link` | Name on the server's front page |
| `ORBIT_LINK_DASHBOARD_BIND` / `_PORT` | `0.0.0.0` / `8080` | Where the dashboard listens. Use `127.0.0.1` with HTTPS |
| `ORBIT_LINK_DOMAIN` | (empty) | Domain for the HTTPS profile |
| `ORBIT_LINK_TRUST_PROXY` | (empty) | `on` behind a reverse proxy (HTTPS profile, Coolify, Traefik) |
| `ORBIT_LINK_CLIENT_IP_HEADER` | (empty) | With `TRUST_PROXY`, the header holding the visitor's address, such as `CF-Connecting-IP` behind Cloudflare |
| `ORBIT_LINK_HTTP` | `0.0.0.0:8080` | Inside the container: where the server's HTTP listens (the compose file maps it) |
| `ORBIT_LINK_DATA` | `/data` | Inside the container: the data folder |

Server data (keys, networks) lives in the `orbit-link-data` volume. Back it up
to keep your networks.

**Coolify or another platform:** build `docker/Dockerfile` (target `server`) or
deploy `docker-compose.yml`, publish UDP 41700 directly (not through the HTTP
proxy), put the dashboard behind the platform's HTTPS, and set
`ORBIT_LINK_TRUST_PROXY=on`. Behind a proxying CDN such as Cloudflare, give the
UDP endpoint its own name that isn't proxied (`ORBIT_LINK_PUBLIC_HOST`), and set
`ORBIT_LINK_CLIENT_IP_HEADER` so attempt limits apply per visitor.

### Running a public server

A public server is an ordinary server with `ORBIT_LINK_OPEN_NETWORKS=on`. Set
`ORBIT_LINK_RELAY=off` to keep bandwidth costs near zero (the server then only
introduces devices), and consider `ORBIT_LINK_PRUNE_DAYS=90` and
`ORBIT_LINK_PRUNE_UNUSED_HOURS=24`, so networks made in bulk and never used don't
fill it. The server limits join and network-creation attempts per visitor and in
total, answers handshakes at a bounded rate (asking for a cookie under load), and
never answers with more data than it received. The Orbit Play helper offers Orbit
Play's server by default; `ORBIT_LINK_PUBLIC_SERVER` at build time changes that
(see [Build and test](#build-and-test)).

## Security

- Traffic between devices is encrypted end to end (WireGuard protocol). The
  server only sees encrypted packets, also when it relays them.
- Each device creates its own key; the private key never leaves the device.
- Only devices in your network can reach your PS5. Removing a device cuts it off
  immediately.
- Join codes have 8 characters, work once and expire after 30 minutes.
  Attempts are rate-limited.
- Inside the Orbit Play helper, Orbit Link has no page of its own: only a
  computer that the helper trusts can set it up or invite devices. Connections
  Orbit Link makes for other devices never get the console's trust-code page.
- `orbit-link-node`'s page only listens on `127.0.0.1` and answers only to its
  own address, so web sites cannot reach it through DNS rebinding.
- **Use HTTPS for your own server's dashboard** (the `https` profile). Without
  it, the admin password and join codes cross the network unencrypted.
- The **payload loader (TCP 9021) is blocked** for your other devices.

## Limits

- **No phone app yet.** Phones cannot join; use a computer.
- Without a relay, a few network combinations cannot connect directly, such as
  strict (symmetric) NAT on both sides, or networks that block UDP between
  devices. A self-hosted server with relay covers them.
- The PS5 has no system VPN, so games and PSN never use Orbit Link. It only
  carries connections made through port forwards.
- On strict networks such as phone hotspots, finding a direct connection can take
  up to about half a minute.
- The PS5 can't be woken from Rest Mode: it must stay on with the helper running.
- FTP passive data connections are not forwarded automatically yet.
- IPv4 only.

## Troubleshooting

| Problem | What to check |
| --- | --- |
| Orbit Play says the PS5 isn't online | The PS5 is on (not in Rest Mode), online and running the Orbit Play helper |
| "Could not connect directly" | The networks block direct connections. Try another network, enable UPnP on your home router, or use your own server with relay |
| "Another program … is using a Remote Play port" | Quit the program using that port on the computer, such as another Remote Play app |
| Code rejected | Codes work once and expire after 30 minutes. Create a new one; an away code comes from the PS5's **Play away from home → Get a code** |
| "Orbit Link could not reach its server" | This computer's internet connection, and with your own server its address |
| "No longer in your PS5's Orbit Link network" | At home, turn play away from home on again |
| The PS5 says "play away from home stopped the helper when it last started" | The helper crashed while Orbit Link started, so its setup was set aside. Turn it on again at home and send the logs |
| Forgotten admin password | Set `ORBIT_LINK_ADMIN_PASSWORD` in `.env` and run `docker compose up -d` |

Logs: PS5 `/data/orbit-play/link/orbit-link.log` and `/data/orbit-play/helper.log`; computer: `link/orbit-link.log`
in Orbit Play's state folder; server `docker compose logs orbit-link`.

## Build and test

```sh
make image    # the PS5 build image shared with the helper (../ps5/tools/Dockerfile.sdk)
make test     # unit tests
make ps5      # the engine library the helper links, in build/ps5
make server   # server container image
make e2e      # end-to-end test (two NAT'd homes, public and own server)
tools/build-nodes.sh   # orbit-link-node for every platform Orbit Play ships on
```

The helper itself builds with `make payload` in `../ps5`; `make test` at the
repository root also runs the helper with the real engine and
[the away-from-home test](../tests/away/run.sh). `orbit-link-node` builds on any
computer with Rust: `cargo build --release -p link-node`.

See [How it works](docs/how-it-works.md) for the design.

## Licences

The device engine, the PS5 side and the node (`crates/link-core`,
`crates/link-proto`, `crates/link-ps5`, `crates/link-node`, `ps5/`) are
GPL-3.0-or-later ([LICENSE](LICENSE)). The server (`crates/link-server`) is
AGPL-3.0-or-later ([LICENSE-AGPL](LICENSE-AGPL)). Third-party components are
listed in [THIRD_PARTY_NOTICES.md](THIRD_PARTY_NOTICES.md).

WireGuard is a registered trademark of Jason A. Donenfeld. Orbit Link is not
affiliated with WireGuard, Sony Interactive Entertainment or PlayStation.
