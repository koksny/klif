# Publishing into this repo

This GitHub tree is the **public** one. Day-to-day work happens in a private workshop with real paths, real
machines and real configuration. Copy a change here only when it is worth other people seeing, and sanitise it
first. The rules are the same for a person and for an agent.

Agents: also read [AGENTS.md](../AGENTS.md). If `AGENTS.local.md` exists on disk, it wins for machine-specific
paths; that file is gitignored on purpose and its contents never go into git.

## Never commit

- Weights: `*.gguf`, `*.safetensors`, `*.onnx`, `*.pt`, `*.bin`
- Runtimes and builds: `*.exe`, `*.dll`, HIP fat blobs, `.bak` binaries, `dist/`, `target/`, `node_modules/`
- **KLIF's own files from a machine that ran it:** `klif.toml` (any filled-in copy), `state.v3.json`, `state.json`,
  `engine.lock`, `control.json`, `instance-id`, `node-cache.json`, `window.json`, `panel.json`, `bench\*.json`
  (publish bench numbers deliberately in a PR, not the raw file), the `logs\` folder, `webview-data\`, `.local\`
- Secrets: `api-key.txt`, `node-token.txt`, `*.token`, `.env*`, `credentials.json`, keys, tokens, cookies, anything
  with an `HF_TOKEN`
- Absolute personal paths (`C:\Users\...`, a private workshop root)
- LAN and WAN IPs, hostnames, computer names, Wi-Fi names, Tailscale/ZeroTier/VPN addresses
- Personal e-mail, Discord, phone, street address
- Other people's unpublished models or paid weights
- `.cursor/` and other editor or agent state
- Screenshots, recordings and pasted terminal output that show any of the above (console lines contain prompts,
  paths and addresses; tab labels and node names can be hostnames)

`.gitignore` is a backstop for the secrets and the engine's own files (`api-key.txt`, `node-token.txt`, `*.token`,
`control.json`, `instance-id`, `engine.lock`, `state.v3.json`). It does not cover everything on this list; the
checklist below does.

## Generalize before copy

| Private workshop | Public tree |
| --- | --- |
| Hardcoded workshop root | `$PSScriptRoot`, an environment variable, or a documented config key |
| Hardcoded weight paths | `{models_dir}` / `{model}` placeholders, or fictional `D:\models\...` in examples |
| Real server locations | Fictional `D:\llama.cpp\llama-server.exe` |
| Real LAN addresses | `127.0.0.1`, or the documentation range `192.0.2.x` (RFC 5737) |
| Real node and System names | `render-box`, `System 1`, `s1` |
| HIP device ordinal from one PC | Leave it out, or say it is machine-specific ([platforms.md](platforms.md)) |
| One-box dual-GPU map | An optional preset, not the only layout |
| Bench notes with hostnames | GPU class, backend, versions and numbers only |
| A model you downloaded by hand | The Hugging Face repo id, a commit sha and file hashes, verified against the hub |

## Allowed in public code

- Branding (KLIF, Koksny.com, the mark)
- Generic AMD GPU class names (for example "RDNA4 16 GiB", `gfx1201`) and PCI ids of public GPU models, as examples
- Default loopback ports (`7030`, `1234`, `8000`, `8080`, `7340`) as documentation
- Hugging Face **repo ids** and commits for models people can download themselves, not local filenames under a
  private folder
- MIT-licensed code, scripts, UI source (not `node_modules`)

## Sanitisation checklist

Run it on the exact change you are about to commit, every time.

1. **Know what is in it.** `git status` and `git diff --stat` (and `git diff --cached --stat` if you staged).
   Nothing unexpected, nothing from the "Never commit" list. New files are the ones to read.
2. **Search the change for personal data.** On the added lines of the diff:

   ```powershell
   $pattern = 'C:\\Users\\|[A-Za-z]:\\(Users|Documents and Settings)\\|\b192\.168\.|\b10\.\d{1,3}\.\d{1,3}\.\d{1,3}\b|\b172\.(1[6-9]|2\d|3[01])\.|\b100\.(6[4-9]|[7-9]\d|1[01]\d|12[0-7])\.|@[A-Za-z0-9.-]+\.[A-Za-z]{2,}|hf_[A-Za-z0-9]{20,}|BEGIN [A-Z ]*PRIVATE KEY|\.gguf|\.safetensors'
   git diff HEAD -U0 -- . ':!docs/publish.md' | Select-String '^\+' | Select-String -Pattern $pattern
   git grep -n -i -e $env:USERNAME -e $env:COMPUTERNAME -- . ':!docs/publish.md'
   ```

   Look at every hit. `.gguf` in an example path under `D:\models` is fine; a path under your profile is not.
   Add anything specific to you: your hostnames, your Tailscale name, your workshop folder, your handle.
3. **New files only (not yet tracked):** `git ls-files --others --exclude-standard` and search them the same way with
   `Select-String -Path`.
4. **Config and docs examples** use only fictional paths, `127.0.0.1` or `192.0.2.x`, and `render-box`-style names.
   A filled-in `klif.toml` is never an example; start from `config/klif.example.toml`.
5. **Presets and recommendations.** No secrets in `args`, no network or model-fetch flags (`-hf`, `--hf-repo`,
   `--model-url`, ...), no `env` in a recommendation, no ordinal that only your machine has. Repo, commit and file
   hashes were checked against the hub (a file with its own `repo` and `revision` against that repo).
6. **Bench output.** GPU class, VRAM, backend and version, numbers. No hostnames, paths or addresses; trim the
   `hardware` block of a pasted record if it names more than the GPU.
7. **Images.** Look at the pixels (console drawer, tab labels, Tune fields, node names) and at the metadata. Redo
   the screenshot with the mock engine (`?scenario=multi`) rather than blurring a real one.
8. **Binaries are not committed.** If you publish one anywhere else: scan it for leaked paths
   (`strings -n 8 klif.exe | Select-String 'C:\\Users|\.cargo|<your user name>'`), read its version information, sign it
   or publish its SHA-256, and scan it with `MpCmdRun.exe -Scan -ScanType 3 -File <exe> -DisableRemediation`. The
   release script strips build paths; check anyway.
9. **Identity.** `git config user.name` and `user.email` are what you want public on this repository.
10. **Push** only to `github.com/koksny/klif`. Never force-push to `main`.

If a file cannot be sanitised without rewriting it, leave it in the workshop.

## How to copy a drop

1. Confirm the change is a product improvement, not a one-machine hack.
2. Diff against this repo; copy files, do not move your editor workspace onto this tree.
3. Apply the substitutions in the table above.
4. Run the checklist.
5. Commit here in small pieces. Push only to `github.com/koksny/klif`.
