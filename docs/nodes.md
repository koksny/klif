# Nodes: several machines

A **node** is a computer running KLIF. One node can show another's Systems next to its own, so a GPU box, a render
machine and a laptop become one stack in one window (and one `klif-cli`). There is no server to install: every node
is the same program, one side listens, the other connects.

Read the security section before you turn the listener on. In short: **`edit` means arbitrary command execution on
that machine, and the traffic is not encrypted yet.** A default install opens no network port.

## Setting it up

On the machine that **offers** its Systems (the node), in its `klif.toml`:

```toml
[node]
name = "Render box"          # shown on the other side; default: the computer's name
listen = "192.0.2.10:7340"   # opt-in. An address of one network interface is better than "0.0.0.0:7340"
allow = ["launch"]           # rights besides view: "launch" and/or "edit"
```

Then create its token (printed once, stored in `node-token.txt` next to its `klif.toml`):

```powershell
klif-cli node token --create
```

Without a token file the listener stays off and the node shows an issue ("[node] listen is set but there is no
node token yet ..."). The node's engine must be running: the KLIF window or `klif-cli serve`. Windows Firewall may
ask whether to allow the program; allow Private networks only, or add a rule scoped to the other machine:

```powershell
# elevated PowerShell; KLIF never changes firewall rules itself
New-NetFirewallRule -DisplayName "KLIF node" -Direction Inbound -Protocol TCP -LocalPort 7340 `
  -RemoteAddress 192.0.2.20 -Profile Private -Action Allow
```

On the machine that **connects**, save the token into a file (or an environment variable) and add the node:

```toml
[nodes.render-box]           # id: [a-z0-9][a-z0-9_-]{0,31}
address = "192.0.2.10:7340"  # default port 7340
token = "file:render-box.token"   # "file:<path>" (relative to this klif.toml's folder, or absolute) or "env:NAME"
name = "Render box"          # display; default: what the node reports
```

The token is never written into `klif.toml`, and KLIF rejects a `token` that is not `file:` or `env:`. The token
file holds the token on one line (at least 32 characters). Check the link:

```powershell
klif-cli nodes list
klif-cli status
```

The node's local Systems now appear after yours, as `render-box/s1`, `render-box/cgi`, ...

## What you see and can do

- **Tabs.** A remote System's tab carries the node's name next to the label and a status; the id is
  `<node>/<system>`. If the node is down, its last known Systems stay as `unreachable` tabs (no session, no
  command) so the layout does not jump.
- **Controls follow the node's rights.** With only view, Launch, Stop and the rest are disabled; with `launch` you
  can launch, stop, restart, dismiss, choose a preset and set params; with `edit` you can also change presets and
  Systems and download models there. Each action is checked by the node on every request.
- **Selecting a remote tab** asks that node for the System's console and session (the focused node is polled at 2 Hz,
  the others at 1 Hz). Selecting never stops anything.
- **In Tune** a remote tab hides Recommended and the API key row (those belong to the node's own klif.toml), takes
  the GPU list from the node, and sends every preset, download and add action to that node.
- **Endpoints** of remote Systems are rewritten so they work from here: when the node reports a loopback or wildcard
  host, the node's address is substituted.
- **Not remote:** `bench` (run `klif-cli` on that machine) and Copy API key (the key never leaves its machine).
- **Conflicts** are computed by each node for its own Systems. A System here and one there never conflict.
- **Presets are per node.** `vm.presets` is this machine's; a remote System uses the preset of the same name *on that
  node*, shown from the node's masked copy.
- **From `klif-cli`:** `status`, `plan render-box/s1`, `launch render-box/s1 --yes --wait`,
  `presets list --node render-box`. A label that is unique across nodes also works (`render cgi`).

## Rights

| Right | Granted by | Allows |
| --- | --- | --- |
| view | always (to anyone with the token) | `hello`, `snapshot`, `status`, `preset` (secrets masked), `plan` (secrets masked), `records_history` |
| `launch` | `allow = ["launch"]` | Launch, Stop, StopAll, Restart, Dismiss, UsePreset, SetParam |
| `edit` (implies `launch`) | `allow = ["edit"]` | SavePreset, DeletePreset, AddSystem, RemoveSystem, UpdateSystem, DownloadRecommendation, CancelDownload, AdoptRecommendation, and `command_preview` |
| never over the network | | Select, `diag`, and anything unknown |

Rights are decided per request, default-deny, and re-read from the node's `klif.toml` about once a second, so
removing a right takes effect for connected peers almost at once. A param change is checked against the preset's
declared choices and cannot widen anyone's rights. `UsePreset` checks that the preset exists and has the right
kind before anything is written; once it goes through it removes the System's param selections that the new
preset does not declare, exactly as it does locally.

A peer sees only the node's **local** Systems: never that node's own remote nodes (a node does not act on its
own nodes for a peer, and refuses actions that name no System), no recommendations, no downloads, no
paths from its config. A System's session and console are sent only for the System the peer focused.

## Security model

Plainly:

- **`edit` is remote code execution on that machine.** A preset is a program plus arguments; whoever may save one
  may run anything as the user KLIF runs as. Grant `edit` only to a machine and a person you would give a shell.
- **`launch` lets a peer start and stop your Systems**, with the commands already in your presets. It cannot change
  them.
- **The listener is opt-in.** `[node] listen` unset means no port is opened. Set but without a token file means
  no port is opened. The local control channel `klif-cli` uses is separate: loopback only.
- **Authentication is a challenge and a keyed hash; the token never crosses the wire.** On connect the node sends a
  random nonce. Every request carries `mac = HMAC-SHA256(token, nonce, id, method, params)`; ids must strictly
  increase on a connection. A request cannot be replayed on another connection (new nonce) or on the same one (id
  order). The comparison is constant-time. The token is 32 random bytes, so guessing it is not realistic; a failed
  attempt also costs the attacker a second and one of four slots.
- **The node proves itself too.** The connecting side sends its own random nonce in `hello`, and the node answers
  with an HMAC over both nonces and its instance id. A listener that does not hold the token cannot answer, so it
  shows as `unauthorized` ("did not prove it holds the node token") and nothing it says is trusted: its Systems never
  appear and it cannot take the place of the real node. Someone relaying a live connection between two real
  machines is still possible until TLS exists.
- **Nothing is encrypted until TLS exists.** Requests are authenticated, not encrypted, and responses are neither.
  Anyone who can watch the traffic between the machines can read everything that crosses: the node's Systems, console
  lines, presets (with secret values masked), and what you send. Someone in the middle of a live connection can
  drop it or show you false answers. Without the token they cannot make the node run anything. Use KLIF nodes
  only on a network you trust, or over a VPN you trust. Do not forward the port to the internet. TLS is future work.
- **Secrets stay out of it.** The KLIF API key and node tokens are never sent. Presets fetched from a node arrive
  masked, and Tune refuses to send a secret environment value to a remote node ("Secrets cannot cross plain TCP;
  set them in that node's klif.toml"). KLIF itself refuses the same for every client (Tune, `klif-cli`, your own
  agent): a preset save or command preview for a remote node that carries a clear secret environment value or a
  secret-flag value (`--api-key`, `--hf-token`, `-hft`, also inside params' choices) is refused before it leaves
  this machine. The mask (`••••`, keep the stored value) and a `{env:NAME}` reference (the node's own environment)
  pass. Tune's live preview of a remote preset sends typed secrets masked.
- **The token file is a password.** It is stored in plain text, on the node (`node-token.txt`) and on every
  machine that connects (the file or variable named by `token`). Keep both inside the user's profile. Rotate
  with `klif-cli node token --create --yes` (the old token stops working for connected peers within about a
  second; give the new one to the others) whenever a machine that held it is lost, reused or compromised.
- **Listener hardening:** the first complete line must arrive within 3 seconds; at most 4 unauthenticated
  connections in total and 2 per peer address (more are closed at once); at most 8 authenticated connections; an
  idle authenticated connection is closed after 30 seconds; TCP keep-alive every 10 seconds; request lines are at
  most 1 MiB (answers at most 16 MiB on the client). A wrong MAC, a bad id or a bad line gets an answer and the
  connection is closed about a second later by a timer, so failing connections do not slow the others down.
- **Identity.** Each installation has a random id (`instance-id`). A `[nodes.*]` entry that turns out to be this
  machine shows as offline with "This is this machine", and one that turns out to be the same installation as an
  earlier entry shows "Duplicate of <node>"; the entry earlier in the file wins. No Systems are taken from either.
- **Versions.** `hello` carries a protocol number (currently 2; 0.3.0 added the node's proof). A node with another
  number is `incompatible`. So is
  a node whose snapshot cannot be read at all; fields missing from an older node's answer fall back to defaults.

What KLIF does **not** do: encrypt, pin certificates, rate-limit by account, or write firewall rules.

## Node states

| State | Meaning |
| --- | --- |
| `connecting` | First connection attempt in progress |
| `online` | Authenticated; polled every second (focused node: twice a second). `latencyMs` is measured per poll |
| `offline` | Could not connect or the node stopped answering (connect timeout 1.5 s, answer timeout 5 s); retried with backoff up to 10 s. Also "This is this machine" and "Duplicate of ..." |
| `unauthorized` | The node rejected the token, the node could not prove it holds the token ("did not prove it holds the node token; it may not be your node"), or the token file/variable is missing or too short |
| `incompatible` | Another protocol number, or an answer that could not be decoded; the error says which |

An action forwarded to a node (launch, stop, usePreset, ...) returns when the node has accepted it, not when it
finished; a forward waits at most 10 seconds. Watch the System's status for the result.

The node's rights are re-read by the connecting side every 5 seconds (`hello`), so its buttons follow `allow` changes
without a restart. Cached Systems of nodes are kept in `<data folder>\node-cache.json`:

```json
{ "version": 1, "nodes": { "render-box": { "at": 1790000000.0,
    "systems": [ { "id": "cgi", "label": "System CGI", "kind": "image", "preset": "image-turbo", "modelName": "Image turbo" } ] } } }
```

It is written when a node's list changes, at most every 30 seconds, and is only used to draw `unreachable` tabs; it
is never used to adopt anything.

## Protocol reference

For people who write their own client (an agent, a dashboard). The same protocol runs on the local control channel
(`127.0.0.1`, token in `control.json`, full rights) and on the network listener.

- **Transport:** TCP, one JSON document per line (`\n`). On connect the server sends one line:
  `{"nonce": "<16 random bytes, hex>", "instanceId": "<hex>"}`.
- **Request:** `{"id": 1, "method": "hello", "params": null, "mac": "<hex>"}`. `id` is a number, strictly greater than
  the previous one on this connection.
- **MAC:** `hex(HMAC-SHA256(key = the token's UTF-8 bytes, message = nonce + "\n" + id + "\n" + method + "\n" +
  canonical(params)))`, where `id` is decimal and `canonical(params)` is the params as JSON with object keys sorted
  and no whitespace (`null` when absent).
- **Response:** `{"id": 1, "ok": true, "result": {...}}` or `{"id": 1, "ok": false, "error": {"code": "...",
  "message": "..."}}`. Responses carry no MAC.
- **Hello:** params `{"nonce": "<16 random bytes, hex>"}` (required on the network listener, optional on the local
  channel). The answer's `proof` is `hex(HMAC-SHA256(key = the token, message = "klif-server\n" + your nonce + "\n" +
  the server's nonce + "\n" + instanceId))`; check it (constant-time) before trusting anything else the server says.
- **Methods:** `hello` (-> `{schemaVersion, klifVersion, nodeName, allow, instanceId, proof}`), `snapshot` (`{focus?}`, the
  view model), `status` (the compact `StatusJson`), `act` (params = the action object, for example
  `{"type": "launch", "system": "s1", "stopOthers": true}`), `plan` (`{system}`), `preset` (`{id}`), `command_preview`
  (`{spec, system?}`), `records_history` (`{key, metric?}`: the broken records of one of this machine's record keys,
  oldest first, as the `RecordEvent`s of the view model; `klif-cli records history` asks the node that keeps an
  entry), `diag` (local channel only).
- **Error codes:** `bad_request`, `unauthorized` (wrong MAC or a rotated token; the connection closes about a second
  later), `bad_id`, `forbidden` (the connection's rights do not cover this method or action), `unknown_method`,
  `bad_params`, `refused` (the engine said no; `message` is its sentence), `too_large`, `busy` (too many
  connections).

`klif-cli --json status` is the same document as the `status` method plus `schemaVersion`.
