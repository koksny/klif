# Agent notes (public)

This repository is the **public** KLIF tree: [github.com/koksny/klif](https://github.com/koksny/klif).

## Do

- Keep commits small and publishable.
- Prefer `$PSScriptRoot` and environment/config over absolute paths.
- Default HTTP bind to `127.0.0.1`.
- Follow [docs/publish.md](docs/publish.md) before adding files that originated in a private workshop.
- If `AGENTS.local.md` exists in the working copy, treat it as the private map (workshop root, hardware, LAN). Do not copy its contents into git.

## Do not

- Do not commit weights, HIP/Vulkan binaries, logs, launcher state, or `.env`.
- Do not put LAN IPs, home hostnames, or personal filesystem roots in tracked files.
- Do not vendor `node_modules` or fat HIP DLLs.
- Do not `git push --force` to `main`.
- Do not switch the operator's daily Cursor workspace onto this repo unless they asked.

## Product voice

Window title **KLIF**. Tagline: **Koksny.com LOCAL INFERENCE FORNICATOR**. Subline: **AMD optimized frontend for transformers and DiT**.
