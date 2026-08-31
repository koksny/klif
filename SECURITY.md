# Security

KLIF runs inference **on the operator's machine**. Treat the launcher like any local server:

- Keep the default bind on loopback.
- Do not expose llama-server / sd-server / ComfyUI to the internet without your own auth and firewall.
- Weights and API tokens are yours; they must never land in this git history.

If you find a vulnerability in **committed** KLIF code, open a private GitHub security advisory on [koksny/klif](https://github.com/koksny/klif) rather than a public issue. Do not attach `.env` files, logs with prompts, or model dumps.
