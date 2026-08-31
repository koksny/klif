# KLIF

**Koksny.com LOCAL INFERENCE FORNICATOR**

AMD optimized frontend for transformers and DiT.

Windows launcher for local inference on AMD GPUs: language models (transformers) and image/video DiT via HIP/ROCm and Vulkan.

## Status

**0.1** — public project identity. The launcher and runtime glue will land here as they stabilize. This tree is the public snapshot; day-to-day work stays private until a change is ready to publish.

## Requirements (preview)

- Windows 10/11
- PowerShell 5.1+
- AMD GPU with a HIP (ROCm) or Vulkan path
- Local GGUF / safetensor weights (not shipped)

## License

MIT. Model weights you load through KLIF keep their own licenses.
