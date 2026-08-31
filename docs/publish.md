# Publishing into this repo

This GitHub tree is the **public snapshot**. Day-to-day work happens in a private workshop. Copy a change here only when it is worth other people seeing.

Agents: also read `AGENTS.md`. If `AGENTS.local.md` exists on disk, it wins for machine-specific paths — that file is gitignored on purpose.

## Never commit

- Weights: `*.gguf`, `*.safetensors`, `*.onnx`, `*.pt`, `*.bin`
- Runtimes: `*.exe`, `*.dll`, HIP fat blobs, `.bak` binaries
- State and logs: `.llm-launcher-state.json`, `runtime-logs/`, `measure-logs/`, `*.log`
- Secrets: `.env*`, `credentials.json`, keys, tokens, cookies
- Absolute personal paths (`C:\Users\…`, a private workshop root)
- LAN / WAN IPs, hostnames, Wi-Fi names, Tailscale/ZeroTier addresses
- Personal email, Discord, phone, street address
- Other people's unpublished models or paid weights
- `node_modules/`, build trees, `.cursor/` user state

## Generalize before copy

| Private workshop | Public tree |
| --- | --- |
| Hardcoded workshop root | `$PSScriptRoot`, `$env:KLIF_ROOT`, or a documented config file |
| Hardcoded weight paths | config / env (`KLIF_MODELS`, per-family vars) |
| Hardcoded LAN bind | `127.0.0.1` default; optional bind address in config |
| HIP ordinal from one PC | detect devices, or document "set `HIP_VISIBLE_DEVICES`" |
| One-box dual-GPU map | optional profile, not the only layout |
| Internal benchmark notes with hostnames | drop, or keep numbers without the machine |

## Allowed in public code

- Branding (KLIF, Koksny.com, the mark)
- Generic AMD GPU class names (e.g. "RDNA4 16 GiB", "gfx1201") as **examples**
- Default loopback ports (`7030`, `1234`, `8188`) as documentation
- Hugging Face **repo ids** for models people can download themselves — not local filenames under a private folder
- MIT-licensed scripts, WPF XAML, web UI source (not `node_modules`)

## How to copy a drop

1. Confirm the change is a product improvement, not a one-machine hack.
2. Diff against this repo; copy files, do not move the Cursor workspace.
3. Run the substitutions in the table above.
4. `git status` / `git diff` and search the diff for `C:\`, `192.168.`, `.gguf` path literals, emails, tokens.
5. Commit here. Push only to `github.com/koksny/klif`.

If a file cannot be sanitized without rewriting it, leave it in the workshop.
