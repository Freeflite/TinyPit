# tinypit

**A tiny zero-window TCP tarpit — accept, never read, dribble, hold.**

`tinypit` answers the TCP handshake, sets a *tiny* receive buffer, then **never reads**. The attacker's probe or exploit fills that buffer, the kernel advertises a **zero window**, and their TCP stack jams in persist-mode — stuck on a socket that will never drain. tinypit only ever dribbles a single space byte now and then (to keep banner-scanners "waiting", and to notice when a peer finally gives up). It never touches the attacker's bytes, so there is **zero parser attack surface**.

Every accepted connection is logged as `<epoch> <source-ip>`, so a banning engine — like [ProBAN](https://github.com/Freeflite/PROBan) — can count the knocks and escalate.

A few hundred lines of Rust / [tokio](https://tokio.rs), split into small focused modules. No config files, no request parsing — nothing but tokio + socket2.

## Why

Dropping a scanner is free for *them* — they move on instantly. Tarpitting isn't: every trapped connection ties up one of their sockets and some of their time, for as long as they'll hold it. tinypit holds a lot of them cheaply (bounded total + per-source caps) and hands a reputation engine the list of who keeps knocking.

## Build

```sh
cargo build --release
# binary: target/release/tinypit
```

**Deploying to a different (or older) Linux than you build on?** Build a fully **static** binary — it links musl instead of glibc, so it runs anywhere regardless of the target's glibc version (e.g. copying from a newer build host onto an older container/DMZ box):

```sh
rustup target add x86_64-unknown-linux-musl
cargo build --release --target x86_64-unknown-linux-musl
# static binary: target/x86_64-unknown-linux-musl/release/tinypit
```

## Run

```sh
./target/release/tinypit    # binds [::]:3333 — dual-stack v4+v6 by default
# v4-only if you prefer:  TINYPIT_LISTEN=0.0.0.0:3333 ./target/release/tinypit
```

Or use the systemd unit: drop the binary at `/usr/local/bin/tinypit`, put `tinypit.service` in `/etc/systemd/system/`, then `systemctl enable --now tinypit`.

### Configuration (environment)

| Var | Default | Meaning |
|---|---|---|
| `TINYPIT_LISTEN` | `[::]:3333` | bind address — `[::]` is dual-stack v4+v6 (the binary forces `IPV6_V6ONLY` off, so it holds even on the BSD v6-only default); use `0.0.0.0:3333` for v4-only |
| `TINYPIT_MAX` | `8192` | max total concurrent connections (global cap) |
| `TINYPIT_PER_IP` | `64` | max concurrent connections per source IP |
| `TINYPIT_DRIBBLE` | `30` | seconds between the 1-byte dribble (`0` = 60s hold) |
| `TINYPIT_RCVBUF` | `256` | receive-buffer bytes — smaller = faster zero window |
| `TINYPIT_LOG` | `/var/log/tinypit/connections.log` | connection log path |
| `TINYPIT_LOG_FORMAT` | `text` | log format: `text` (space separated, default) or `json` (for structured logging) |

### CLI (startup only, before listening)

```sh
tinypit --version     # prints version
tinypit /?            # prints version + "There are no flags available..."
tinypit --help
```

There are deliberately no runtime flags. All configuration is via environment variables. This keeps the attack surface minimal.

### Connection log

One line per accepted connection (text format by default):

```
1720051200 203.0.113.7
└─ epoch    └─ source IP
```

With `TINYPIT_LOG_FORMAT=json`:

```json
{"ts":1720051200,"ip":"203.0.113.7"}
```

Point your ban / reputation engine at this file. Flexible format makes it easy to ingest.

## ⚠️ Safety — run it isolated

tinypit is a magnet for hostile traffic and it **holds attacker connections open on purpose**. Run it on an **isolated host or container in a DMZ**, never on your router/firewall itself, and expose only the port(s) you're baiting. It only ever listens, holds, and dribbles — it never initiates connections and never reads attacker input, so it can't be turned into a client — but treat the box it runs on as hostile-adjacent and segment it accordingly.

## Pairs with ProBAN

tinypit is the companion tarpit for [**ProBAN**](https://github.com/Freeflite/PROBan), a progressive-ban plugin for OPNsense: ProBAN redirects banned IPs into tinypit and counts their continued knocks to escalate the ban. tinypit stands alone fine too — anything that can read a `<epoch> <ip>` log can drive off it.

## License

BSD 2-Clause — see [LICENSE](LICENSE).

**Attribution to [github.com/Freeflite/TinyPit](https://github.com/Freeflite/TinyPit) is required** if used in any derivative work, product, package, or distribution.
