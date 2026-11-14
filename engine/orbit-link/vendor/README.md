# Vendored crates

## boringtun 0.7.1

Cloudflare's WireGuard implementation (BSD-3-Clause), extracted from the
crates.io archive `boringtun-0.7.1.crate`
(SHA-256 `15dd6a8a89cbe8997f37ca0cf035e6ea4d64cd2ecea4aed83ffb9f99f7126939`).

The only changes are in `Cargo.toml`: `crate-type` is reduced to `rlib`, and a
`[lints.rust]` table silences warnings from the unmodified upstream code. Upstream
also builds `staticlib` and `cdylib`, which makes Cargo link a shared library
for every target; the PS5 payload toolchain has no shared FreeBSD system
libraries to link one against. No source file is modified.
