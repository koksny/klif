# Overview

KLIF is a Windows frontend that starts local inference servers and points a UI at them. It does not train models and it does not vendor weights.

## Intended layout (when code is published)

```
klif/
  Launch-LLM.ps1          # profile catalog, start/stop, HTTP bind
  Launch-LLM.Gui.ps1      # WPF shell
  Launch-LLM-GUI.vbs      # double-click entry
  launcher-ui/            # mark, assets
  docs/
  llama/                  # llama-server wrappers (HIP / Vulkan)
  sd-server/              # image + video DiT (HIP), web UI source
```

Exact folder names may change; this is the shape, not a promise of paths.

## Roles

| Role | Backend | Typical local port |
| --- | --- | --- |
| Language / agent | llama-server, HIP or Vulkan | `127.0.0.1:7030` (configurable) |
| Image (Krea-class turbo) | sd-server, HIP | `127.0.0.1:1234` |
| Video (MiniMax H3-class) | sd-server, HIP | `127.0.0.1:1234` |
| Video (Comfy graph, optional) | ComfyUI | `127.0.0.1:8188` |

Defaults must bind to loopback unless the operator opts into LAN. Never bake a home LAN address into committed code.

## Hardware the public tree should talk about

Document **classes**, not a single living-room PC:

- HIP on RDNA4-class 16 GiB (example: RX 9070 XT / `gfx1201`)
- Vulkan fallback on the same card
- Optional second card only as a documented dual path, not as a required layout
- CPU / system RAM for encoder offload when DiT VRAM is tight

Do not commit HIP device ordinals from one machine (`ROCm0` vs `ROCm2`) as if they were universal.

## What users must supply

- GGUF / safetensor weights
- A HIP or Vulkan `llama-server` / `sd-server` binary they built or obtained themselves
- A config or environment pointing at those files (no `C:\…` hardcodes in the public tree)

See [publish.md](publish.md) for the copy checklist when promoting a private change into this repo.
