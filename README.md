# 🚫 `portkill`

> **The zero-bloat, lightning-fast Rust TUI for when you can never remember `lsof` syntax.**

[![License: MIT](https://img.shields.io/badge/License-MIT-yellow.svg)](https://opensource.org/licenses/MIT)
[![Language: Rust](https://img.shields.io/badge/Language-Rust-orange.svg)](https://www.rust-lang.org/)
[![Build: Zero Bloat](https://img.shields.io/badge/Bloat-Zero-brightgreen.svg)]()

---

## 💡 Why `portkill` Exists

I’ve been in tech for almost two decades. I’ve configured thousands of servers, deployed countless stacks, and built system architectures from scratch.

And yet, every single time a dev server crashes while holding onto port `8080`, **I still have to Google `lsof -i :8080`**.

Is it `awk '{print $2}'`? Do I pipe it into `xargs kill -9`? What if it's on Linux and I need `netstat -tulpn` or `ss -tulpn`? Life is too short to memorize obscure terminal flags for the 500th time.

So I built **`portkill`**:

- ⚡ **Instant (<5ms launch time)** because it’s compiled Rust, not Electron or Python.
- 🪶 **Sub-5MB binary & zero memory bloat.**
- 🎯 **Single-purpose simplicity.** Type a port number, see what's hogging it, kill it, move on with your day.

Yes, there are a hundred port managers on GitHub. **This one is mine, it's open-source (MIT), and it never forgets the commands so I don't have to.**

---

## ⚡ Quick Start

### 1. Direct Target Mode (Quick Kill)

When you already know which port is blocked:

```bash
# Inspect & interactively kill whatever is on port 8080
portkill 8080

# Force kill immediately without prompt
portkill 3000 --force
```

```
$ portkill 8080
PORT   PROTO PID     PROCESS USER  MEMORY
8080   TCP   39865   node    cyrus 42 M

Send SIGTERM to node (PID 39865)? [y/N] y
Sent SIGTERM to node (PID 39865).
```

| Flag | Effect |
| --- | --- |
| *(none)* | Show what's there, ask, then `SIGTERM` |
| `-y`, `--yes` | Skip the prompt, still `SIGTERM` |
| `-f`, `--force` | Skip the prompt, send `SIGKILL` |

Exit codes: `0` killed · `1` nothing on that port · `2` you said no ·
`3` found it but couldn't signal it.

### 2. Browse Mode (TUI)

Run it bare when you don't know which port you're looking for:

```bash
portkill
```

```
 PORTKILL  41 listening                                              v0.1.0
 PORT   PROTO PID      PROCESS                        USER      MEMORY
 3306   TCP   1685     mysqld                         cyrus      7.2 M
▌6379   TCP   1331     redis-server                   cyrus      1.4 M
 11434  TCP   1626     ollama                         cyrus       12 M
 18789  TCP   1328     node                           cyrus       32 M
 j/k move · / search · ⏎ SIGTERM · K SIGKILL · r refresh · q quit
```

| Key | Action |
| --- | --- |
| `j` / `k`, `↓` / `↑` | Move |
| `g` / `G`, `Home` / `End` | Jump to top / bottom |
| `Ctrl-d` / `Ctrl-u` | Jump 10 rows down / up |
| `/` | Fuzzy filter by port, process, or user |
| `Enter` | `SIGTERM` the selected process |
| `K` (Shift+K) | `SIGKILL` the selected process |
| `r` | Rescan |
| `q`, `Esc`, `Ctrl-c` | Quit |

While filtering, `Enter` keeps the filter and hands the keyboard back to the
bindings above; `Esc` clears it. `↑`/`↓` still aim while you type.

> **On `k`:** vim muscle memory wins — lowercase `k` moves *up*. Killing is
> `Enter` (graceful) and Shift-`K` (force), so a stray `k` can never kill
> anything.

---

## 📦 Install

```bash
cargo install --git https://github.com/jcyrus/portkill
```

Or from a clone:

```bash
cargo install --path .          # into ~/.cargo/bin
cargo build --release           # or just ./target/release/portkill
```

Requires Rust 1.88+. Linux and macOS.

> Not on crates.io: the name `portkill` there belongs to an unrelated project,
> so `cargo install portkill` would fetch someone else's tool, not this one.

---

## 📐 Does it actually deliver?

Release build on Apple Silicon, `hyperfine -N`, 700+ runs, ~45 listening
sockets. Your numbers will differ, but the shape shouldn't:

| Claim | Measured |
| --- | --- |
| Launch time | **2.6 ms** to start; **4.9 ms** including a full system-wide socket scan |
| Binary size | **672 KB** stripped (`aarch64-apple-darwin`) |
| Idle CPU in the TUI | **0%** — the event loop blocks on input, there is no polling tick |
| Runtime dependencies | none beyond libc; no daemon, no config file, no network |

---

## 🔍 How it works

No shelling out to `lsof`, `ss`, or `netstat` — parsing another tool's output
is exactly the fragility this exists to avoid. Instead:

- **Sockets** come from [`netstat2`](https://crates.io/crates/netstat2): netlink
  `sock_diag` on Linux, `libproc` on macOS. Either way the PID behind a socket
  is found by walking open file descriptors, which is why you only see
  processes you have the rights to inspect.
- **Process details** come straight from the OS: `proc_pidinfo` on macOS,
  `/proc/<pid>/status` on Linux. Only the PIDs that actually hold a socket get
  looked up, so there's no whole-system process scan to pay for.
- **Killing** is a plain `kill(2)`, with guardrails: portkill refuses to signal
  PID 0, PID 1, or itself.

TCP sockets are listed only in the `LISTEN` state — an established connection
isn't what's holding your port. UDP sockets are listed when bound.

### Platform notes

Linux and macOS. Because a socket is tied back to its process through that
process's open file descriptors, an unprivileged run mostly shows ports you
own — other users' listeners may be missing entirely, or appear with `?` for
name and user. Run `sudo portkill` to see the rest.

---

## 📄 License

MIT — see [LICENSE](LICENSE).
