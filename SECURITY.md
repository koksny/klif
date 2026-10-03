# Security

KLIF runs inference **on the operator's machine**, as the operator, and by design starts the programs you tell it
to. Treat it like a shell with a nicer window. This page states what KLIF trusts, what it protects, and how to
report a problem. The network side has its own page: [docs/nodes.md](docs/nodes.md).

## Trust model

- **KLIF runs with your rights and asks for no elevation.** The executables carry an `asInvoker` manifest. KLIF
  starts servers as child processes of the same user, hidden, with their output appended to log files. It never
  starts a shell or script host itself; it starts the exact program in the preset.
- **`klif.toml` is as trusted as a script.** A preset is a program plus arguments and environment. Whoever can
  write your `klif.toml` or your KLIF state folder can run programs as you. Keep them in your profile (the default,
  `%APPDATA%\KLIF`), and do not give other accounts write access.
- **Servers are not sandboxed.** A model server KLIF starts has all your rights. KLIF does not inspect model files.
  Model weights and server binaries come from third parties; get them from sources you trust.
- **KLIF never touches a process it does not own.** Stop works on the job object KLIF created for a session. A port
  held by another program is reported, not freed.
- **The local control channel** (`klif-cli` talking to a running KLIF) listens on `127.0.0.1` on a random port. The
  per-run token is in `control.json` in the state folder, and every request is authenticated with an HMAC under
  that token (the token itself is never sent). Any process of your user account can read that file, which is
  exactly as much power as it already has by starting programs itself. Do not put the state folder where other
  accounts can read it.
- **The network listener is off by default.** A default install opens no port. With `[node] listen` set, other
  machines with the node token can view and, if you allow it, control this one. `launch` lets them start and stop
  your Systems; `edit` lets them run arbitrary commands. Traffic is authenticated in both directions (the node
  proves it holds the token too, so a listener without it cannot pose as your node) but not encrypted; someone
  relaying a live connection between two real machines needs TLS to stop, which is future work. Details, limits
  and the protocol: [docs/nodes.md](docs/nodes.md).
- **Secrets.** The KLIF API key (`api-key.txt`, or an environment variable) is passed to llama.cpp and vLLM servers
  that KLIF starts through their key environment variable, not through the command line; inherited copies of
  those variables are removed from the child's environment. Values whose names contain KEY, TOKEN, SECRET, PASS
  or AUTH, and the values after `--api-key` and `--hf-token`, are masked (`••••`) in the window, in `klif-cli`
  output, in logs and in diagnostics. A secret that you write into `args` is visible to every local process and
  KLIF warns about it. Node tokens never cross the network. KLIF refuses to send a clear secret environment value
  or secret-flag value to a remote node (a preset save or command preview from Tune, `klif-cli` or any other
  client); the mask and a `{env:NAME}` reference pass. Tune's live preview of a remote preset sends typed secrets
  masked. `KLIF_LOG=trace` covers KLIF's own records only: dependencies are capped at debug, so request bytes and
  `Authorization` headers are never logged.
- **Downloads** happen only when you ask. KLIF fetches only from `huggingface.co` (and its own CDN hosts
  `*.huggingface.co` and `*.hf.co`) over https, follows redirects only to those hosts, sends a Hugging Face token
  only to `huggingface.co`, and checks each file's SHA-256 against the hub before the file gets its final name.
  That proves the file is what the hub serves, not that the model is safe.
- **Binaries.** Builds from this source are unsigned unless you sign them (`Build-Release.ps1 -Sign`). Verify the
  SHA-256 the build prints. See the Defender section of the [README](README.md).

## What you should do

- Keep servers on `127.0.0.1` unless you need the LAN, and keep the API key on when you do not.
- Do not expose llama-server, sd-server, ComfyUI or a KLIF node to the internet. Put your own authentication and
  firewall in front of anything that must be reachable, or use a VPN you trust.
- Give a node only the rights it needs. Never give `edit` to a machine or a person you would not give a shell on
  this computer.
- Rotate the node token (`klif-cli node token --create --yes`) when a machine that held it is lost or reused.
- Keep weights, tokens, logs with prompts and `klif.toml` out of git history.

## Reporting a vulnerability

If you find a vulnerability in **committed** KLIF code, open a private GitHub security advisory on
[koksny/klif](https://github.com/koksny/klif) (Security tab, "Report a vulnerability") rather than a public issue.
Include:

- what you found, the version (`klif-cli --version`) and the commit if you built it;
- steps to reproduce on a scratch configuration, and what an attacker gains;
- whether it needs a node token, an `allow` right, or local access to the state folder.

Do not attach `.env` files, tokens, `api-key.txt`, `control.json`, `node-token.txt`, logs with prompts, or model
dumps. This is a one-person project: expect an answer when there is one, with no guaranteed time and no bounty.
Fixes go to the latest version only.

In scope: the KLIF code in this repository (the engine, the control and node protocol, the download code, the Tauri
shell, klif-cli). Out of scope: vulnerabilities in third-party servers and runtimes (report them to those
projects), model files, and behaviour documented above as intended, such as a node with `edit` allowed running
commands for a peer that holds its token.
