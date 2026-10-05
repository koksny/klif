# klif-webui: KLIF on a phone

klif-webui is a small page for a phone or a browser on your network. It shows every System with its status, and lets
you launch, stop, restart and dismiss a System, choose its preset and params, and read its console. It is a remote
control for the stack, not a second copy of the window: no Tune drawer, no editing, no downloads.

The page wears the skin the KLIF window shows (colours and fonts only), is plain on purpose, and has no animations.
It is served by KLIF itself, over plain HTTP. **Read the security section before you turn it on.** In short: it is off
by default, only a paired device sees or controls anything, and the connection is authenticated but not encrypted, so
use it on a network you trust and never forward its port to the internet.

## What the page shows

- **The stack.** Each machine (this one first, then every [node](nodes.md)), its GPUs as memory bars that name who
  holds what, and the Systems as tiles under the GPU they run on. The header says how many Systems run and how many are
  in fault.
- **A sheet per System** (tap a tile). Status, model, quant, GPUs, VRAM, speed or load progress, uptime or the last
  session, and the reason when it cannot launch. Below it:
  - **Preset** and every **param** of the preset. A change is saved at once and applies at the next launch (a running
    System shows a note to restart it).
  - The main button: **Launch**, **Launch again** after a fault, **Stop**, or **Stop X & launch** when launching has
    to stop Systems that conflict (port, `exclusive` GPU, VRAM). The button names them and says they will stop, as in
    the window.
  - **Restart** (a running System), **Dismiss** (a fault), **Console** (read only: the last 80 lines, refreshed every
    2 seconds while it is open) and **Close**.
- **Footer.** The name this device paired with, and **Unpair this device**.

The page asks KLIF for its state every 1.5 seconds, and stops while the browser tab is hidden. When KLIF does not
answer it says so and keeps trying.

## Turning it on

klif-webui is off until you turn it on. Three ways, all the same setting:

- **Tune > Web UI** in the KLIF window: the switch, the address and port with **Save**, **Pair a device**, and the
  paired devices.
- `[webui]` in `klif.toml` (edits are picked up while KLIF runs):

  ```toml
  [webui]
  enabled = true
  host = "0.0.0.0"     # every network of this machine (default); or the address of one network, "192.0.2.10"
  port = 7341          # default
  ```

- `klif-cli webui on` (and `--host <ip>`, `--port <n>`), see [cli.md](cli.md#webui).

**It runs in the window app only.** The engine of `klif.exe` serves the page, because the page's files are built into
the app. An engine that `klif-cli` runs for a command, or `klif-cli serve`, serves no page: `klif-cli webui` then
shows `enabled` with `listening: false` and an `error` that says so. Keep `klif.exe` running while you want to use
the page; closing it stops the page (model servers are not touched).

Details:

- **Address.** `host` must be an IP address: `0.0.0.0` (every network of this machine) or the address of one
  interface, IPv6 included (`::`, `::1`). Anything else is reported as an issue and KLIF uses `0.0.0.0`; port `0` is
  reported and replaced by 7341. `127.0.0.1` makes the page reachable from this computer only, which is enough to
  look at it.
- **What to open.** KLIF shows the address a device should open, for example `http://192.0.2.10:7341/`. With
  `0.0.0.0` it is the address of the interface this machine would use to reach another network (KLIF asks the
  operating system; nothing is sent). With several networks, the page is also reachable on the other addresses.
- **The first time.** Windows Defender Firewall asks about `klif.exe` when it first listens on a non-loopback
  address. See [Windows Defender Firewall](#windows-defender-firewall).
- **If it cannot listen** (port taken, no such address), KLIF says why under the switch and tries again every 5
  seconds.

## Pairing a device

A device has to be paired before it sees or controls anything. Pairing proves that someone at the computer allowed it.

1. In Tune > Web UI choose **Pair a device** (or run `klif-cli webui pair --yes`). KLIF shows a QR code and a
   6-digit code, valid for **5 minutes**.
2. On the phone, scan the QR code: the camera opens the page, which pairs at once. Or open the page's address in the
   browser and type the code.
3. The device is listed in Tune with the name it chose for itself ("iPhone", "Firefox on Windows"), and the pairing
   closes.

A pairing is **closed by one success, by 5 wrong codes, or after 5 minutes**; **Cancel** (`klif-cli webui cancel`)
closes it earlier, opening another replaces the open one, and turning the page off or moving it to another address or
port closes it too. A wrong code also costs the caller half a second. The 5-code limit counts every caller together:
someone else on the network who sends wrong codes can close your pairing (never guess it), and you then open a new
one. At most 32 devices can be paired. The code and the QR code are a credential for those minutes: show them only to the
person who pairs.

Pairing needs the page to be served (see [Turning it on](#turning-it-on)). The device keeps a token in the
browser's local storage. A private window, or a browser that blocks storage, has to pair again on every visit.

## Devices

Tune lists each paired device with when it paired and when it was last seen. `klif-cli webui` lists the same with an
id. "Last seen" is written to disk at most once a minute, so it can lag.

**Remove** (or `klif-cli webui forget <id> --yes`, `forget --all --yes`) unpairs a device at once: its next request
is refused and its page returns to the pairing screen. A device can also unpair itself from the footer of the page.
Remove a device whenever the phone is lost, sold or lent out. Deleting `webui-devices.json` in the state folder while
KLIF is closed unpairs every device.

## What a paired device may and may not do

A paired device can do what a [node](nodes.md#rights) with the `launch` right can do (except stop every System at once), on every System the window shows:

| May | May not |
| --- | --- |
| See every machine, GPU and System with status, model, preset, params and speed | Select a tab, add, remove or edit Systems, presets or `klif.toml` |
| Launch (with "stop the Systems that conflict"), stop, restart, dismiss a fault | Download models, adopt recommendations, change settings, the API key or node tokens |
| Choose a System's preset and its params | Open a pairing, or see or remove other devices (Tune and `klif-cli` only) |
| Read a System's console (last 80 lines) | Launch, stop or restart an external server (KLIF only watches those) |

- **Systems on another machine** need that node's `launch` right too. Without it the page shows them and says KLIF
  may only watch them. The console of a System on another machine may be empty.
- **Every paired device is equal.** There are no per-device rights: pair only what you would trust with Launch and
  Stop on your GPU.
- **What it sees is limited, but not secret from a paired device.** The state has no launch commands, no
  `klif.toml` paths and no secrets, but a System's `reason` is KLIF's own sentence and can name a file ("The program
  D:\llama.cpp\llama-server.exe does not exist."). The console is the engine's console for that System without
  KLIF's lines that spell out the launch command and the log files' paths: whatever the server prints (prompts and file
  names included). Lines that mention an API key are replaced by a marker. Treat a paired device as able to read all
  of that.

## Security model

Plainly:

- **Off by default, and then no port is opened.** Turning it on listens on `host:port` until you turn it off.
- **Anyone who can reach the port can open the page**, and sees the pairing screen and nothing else. The page's
  files are served without a token; every `/api/` request needs one, except pairing itself.
- **A device token is 32 random bytes** the engine hands out once, at pairing. The page keeps it in the browser's
  local storage and sends it as `Authorization: Bearer <token>`. There is **no cookie**, so nothing is sent
  automatically for another website to ride on. KLIF stores only the token's **SHA-256** in `webui-devices.json` (state
  folder) with the device's name and dates, so the file alone cannot be used to act as a device. Tokens are compared
  in constant time.
- **The pairing secret stays out of requests and logs.** The QR code carries the page's address with a 32-byte
  secret in the `#` part, which a browser never sends in a request line. The page reads it, removes it from the
  address bar at once and pairs with it. The 6-digit code is the typed alternative.
- **Plain HTTP: authenticated, not encrypted.** This is the same limit as [nodes](nodes.md). Anyone who can watch the
  traffic on your network can read the state, the console lines and the pairing secret, and **can copy a device token
  from a request and control KLIF with it until you remove that device**. Use it on a network you trust (a home
  network with a password you chose, not a hotel, a cafe or a guest network), or over a VPN you trust. **Never forward
  the port to the internet.** TLS is future work.
- **Host header check.** A request must name this machine by an IP address, `localhost` or this computer's name
  (also with `.local`); any other name is answered with 421. This stops a web page on another site from reaching KLIF
  through DNS rebinding.
- **JSON only.** A `POST` must be `application/json`; a form from another site cannot send that.
- **Limits.** At most 16 connections at once and 8 per peer address (more are closed at once), 10 seconds to send a
  request, 16 KiB of headers and 64 KiB of body, one request per connection.
- **Headers.** Every answer carries `X-Content-Type-Options: nosniff`, `Referrer-Policy: no-referrer`, no framing and
  a same-origin resource policy. The page also has a Content-Security-Policy: scripts, fonts and connections only from
  KLIF itself (inline styles and `data:` images are allowed), no forms, no frames, no plug-ins.
- **Pairing and device management are local.** Opening or cancelling a pairing, removing a device and changing
  `[webui]` work only from the window and `klif-cli` on this machine, never over the network. A device can remove
  only itself.
- **Logged** at info level: each pairing and wrong code with the caller's address, each action with the device
  that sent it, a device unpairing itself; a pairing closed by wrong codes is a warning.

What klif-webui does **not** do: encrypt, give devices different rights, limit by address, rate-limit beyond the pairing
attempts, or write firewall rules.

## Windows Defender Firewall

The first time `klif.exe` listens on a non-loopback address, Windows Defender Firewall asks whether to allow it.
**KLIF never changes firewall rules; you decide.** Allow **Private networks** only. To be stricter, add a rule that
admits only your phone (or your network) and say no to the prompt:

```powershell
# elevated PowerShell; KLIF never changes firewall rules itself
New-NetFirewallRule -DisplayName "KLIF klif-webui" -Direction Inbound -Protocol TCP -LocalPort 7341 `
  -RemoteAddress 192.0.2.30 -Profile Private -Action Allow      # one phone
# -RemoteAddress 192.0.2.0/24                                   # or the whole home network
```

Use the port you set in `[webui] port`. With `host = "127.0.0.1"` nothing leaves the computer and Windows asks nothing.

On a Mac, `KLIF.app` serves the page the same way; when the macOS firewall is on, it asks once whether KLIF may accept
incoming connections ([platforms.md](platforms.md#firewall)).

## Troubleshooting

| Symptom | Cause and fix |
| --- | --- |
| The switch is on, nothing answers | Read the line under the switch (Tune) or the `error` of `klif-cli webui`. "Port 7341 is taken by another program": choose another port. "This machine has no address ...": use `0.0.0.0` or an address of this machine (`ipconfig`). "... runs in the KLIF window app, not in klif-cli": start `klif.exe`. |
| No address or QR code is shown | KLIF found no route to a network (no cable, no Wi-Fi). Connect, or open `http://<this computer's address>:7341/` by hand and type the code. |
| The phone cannot open the page | It must be on the same network as the computer: not a guest network, not a Wi-Fi with client isolation (the router calls it AP or client isolation), not through a VPN on the phone. Then check the firewall rule, and that `host` is `0.0.0.0` or the address of the network the phone is on. |
| 421 "answers only to this machine's address or name" | The address in the browser is a name other than this computer's (a proxy, a DNS alias). Use the IP address or the computer's name. |
| "This browser is not paired with KLIF" | The device was removed (Tune, `klif-cli webui forget`, or "Unpair this device"), `webui-devices.json` was deleted, or the browser's storage was cleared. Pair it again. |
| "No pairing is open", "That code is wrong or has expired" | The pairing ran out (5 minutes), was cancelled, or took a wrong code too often (5 closes it). Open a new one. |
| "32 devices are paired already" | Remove one in Tune. |
| The page says KLIF is not answering | The computer is asleep, `klif.exe` was closed or the network dropped. The page keeps trying. |
| The browser calls the page "not secure" | It is plain HTTP, as documented. Do not enter anything else into it. |

## API reference

For people who write their own client. JSON over HTTP/1.x, one request per connection (`Connection: close`; send a
`Content-Length`, chunked bodies are refused with 411). The page itself uses only these.

| Request | Auth | Answer |
| --- | --- | --- |
| `GET /`, `GET /assets/<file>` | none | The page, its scripts, styles and fonts, and its icon |
| `POST /api/pair` `{"secret", "name"}` | none | `{"token", "device": {"id", "name"}}` |
| `GET /api/state` | Bearer | The state, see below |
| `GET /api/console?system=<id>` | Bearer | `{"system", "lines": [...]}`: the last 80 console lines |
| `POST /api/act` `{"type", "system", ...}` | Bearer | `{"ok": true}` |
| `POST /api/forget` `{}` | Bearer | `{"ok": true}`: this device unpairs itself |

Send the token as `Authorization: Bearer <64 hex characters>`. Every `POST` needs `Content-Type: application/json`
and a JSON object as its body.

**Pairing.** `secret` is the 6-digit code (spaces and dashes are ignored) or the 64-character secret after `#pair=` in
the QR code's address. `name` is the device's name (up to 48 printable characters; "Browser" when empty). Keep the
token: it is shown once.

```
POST /api/pair
{"secret": "123456", "name": "Example phone"}

200 {"token": "<64 hex characters>", "device": {"id": "1a2b3c4d", "name": "Example phone"}}
403 {"error": {"code": "pair", "message": "That code is wrong or has expired. Check it in KLIF (Tune, Web UI)."}}
```

**Actions** (`POST /api/act`). Every action names a System by its id (`s1`, `render-box/s1`):

| `type` | Other fields | Does |
| --- | --- | --- |
| `launch` | `stopOthers` (boolean, default `false`) | Launch; with `stopOthers` the Systems that conflict are stopped first |
| `stop`, `restart`, `dismiss` | | Stop, restart, or leave a fault |
| `usePreset` | `preset` (preset id) | The System uses that preset from its next launch |
| `setParam` | `name`, `value` | Choose one of the param's choices, applied at the next launch |

```
POST /api/act
{"type": "launch", "system": "s2", "stopOthers": false}

200 {"ok": true}
```

The answer comes when KLIF has accepted the action, not when it finished: poll `/api/state` for the status. Any other
`type` is refused with 400.

**Errors** are `{"error": {"code", "message"}}` with a sentence in `message`:

| Status | `code` | When |
| --- | --- | --- |
| 400, 415 | `bad_request` | The body is not a JSON object, is not `application/json`, or the action is incomplete or unknown |
| 401 | `unpaired` | No token, or a token that is not (or no longer) paired |
| 403 | `pair` | Wrong code or secret, or no pairing is open |
| 403 | `forbidden` | The System's machine does not grant `launch` |
| 404 | `not_found` | No such System |
| 405 | `method` | Another method on an `/api/` path |
| 409 | `refused` | KLIF refused (a conflict, an external server, a preset that does not fit); `message` says why. Also `pair` with 409 when 32 devices are paired |
| 500 | `io`, `pair` | KLIF could not write `webui-devices.json` |

Outside `/api/`: 404 for an unknown path, 421 for a Host that is not this machine, 431, 413 and 411 for requests that
are too large or lack a length, 503 for the page when this KLIF has no page built in.

**State.** `GET /api/state` returns one object; a field without a value is left out.

```json
{
  "v": 1,
  "now": 1791100000.0,
  "skin": "cliff",
  "onConflict": "ask",
  "device": { "id": "1a2b3c4d", "name": "Example phone" },
  "machines": [
    { "id": "local", "name": "Desk", "local": true, "os": "Windows", "state": "online",
      "gpus": [ { "id": "1002:7550", "name": "RX 9070 XT", "totalGiB": 15.9, "usedGiB": 11.2, "otherGiB": 0.4,
                  "unified": false, "parts": [ { "system": "s1", "label": "System 1", "gib": 10.8 } ] } ] }
  ],
  "systems": [
    { "id": "s1", "label": "System 1", "machine": "local", "kind": "llm", "class": "fast", "status": "online",
      "model": "Fast 8B", "quant": "Q4_K_M", "preset": "fast-8b",
      "presets": [ { "id": "fast-8b", "name": "Fast 8B", "ready": true } ],
      "params": [ { "name": "reasoning", "label": "Reasoning", "value": "on",
                    "choices": [ { "value": "on", "label": "On" }, { "value": "off", "label": "Off" } ] } ],
      "gpus": [ "1002:7550" ], "gpuNames": [ "RX 9070 XT" ], "vramGiB": 10.8,
      "metric": { "v": 62.5, "u": "tok/s" }, "uptimeS": 3600, "conflicts": [],
      "controllable": true, "external": false }
  ]
}
```

- `now`: KLIF's clock in Unix seconds. `skin`: the skin id the window shows (`[ui] skin`). `onConflict`: `ask` or `stop`
  (`[launch] on_conflict`).
- `machines[]`: this machine (`id` `local`) and each node. `state`: `connecting online offline unauthorized
  incompatible`; `latencyMs` and `error` for a node; `os` only for this machine. `gpus[]` give `totalGiB`, `usedGiB`,
  what no System on that GPU alone accounts for (`otherGiB`), whether the memory is the unified memory of an APU
  (`unified`) and the memory each System holds (`parts[]`).
- `systems[]`: `machine` is the machine's `id`; `kind` is `llm image tts stt video music`; `status` is `not-set invalid
  offline starting online busy stopping fault unreachable`; `reason` explains a status or a fault; `presets[]` are the
  presets of the System's kind on its machine (at most 64) with `ready`; `params[]` as in the window (`value` is the
  selected choice); `gpus` are GPU ids (`VEN:DEV`, or `cpu`) and `gpuNames` their names; `vramGiB` is what the
  System holds while it runs and what it is expected to need otherwise; `metric` is the current decode speed or the
  last image's seconds; `last` is a sentence about the last session ("41.3 tok/s · 2 h ago"); `load` is
  `{step, pct}` while a model loads; `uptimeS`; `conflicts` are the labels of the running Systems a launch would stop;
  `controllable` is false when the machine does not grant `launch`; `external` marks a server KLIF only watches.
- It carries no launch commands, no `klif.toml` paths and no secrets; `reason` and the console can name files, see
  above.

The engine is the same one that runs the window: `klif-cli --json status` shows the Systems from the machine side,
and `klif-cli webui` the klif-webui settings and devices.
