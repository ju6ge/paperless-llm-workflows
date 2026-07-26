# Configuration Reference

paperless-llm-workflows supports three configuration sources, applied in layered priority from lowest to highest:

1. **TOML config file** at `/etc/paperless-field-extractor/config.toml`
2. **Environment variables** (each option has a corresponding env var)
3. **CLI flags** (override everything)

A TOML config file is optional — all options can be set via environment variables or CLI flags.

## Required Options

These must be provided at runtime (via env, CLI, or config file):

| Option | Env Variable | Description |
|---|---|---|
| `paperless_server` | `PAPERLESS_SERVER` | URL of your paperless-ngx instance (e.g. `https://paperless.example.com`) |
| `model` | `GGUF_MODEL_PATH` | Path to the GGUF model file (inside container: `/srv/model/model.gguf`) |
| `PAPERLESS_API_CLIENT_API_TOKEN` | — | API token for authenticating with paperless-ngx (not a config option — set as env var directly) |

## Server Options

| Option | Env Variable | Default | Description |
|---|---|---|---|
| `host` | `PAPERLESS_WEBHOOK_HOST` | `0.0.0.0` | Bind address for the webhook API server |
| `port` | `PAPERLESS_WEBHOOK_PORT` | `8123` | Port for the webhook API server |
| `webhook_public_base_url` | `WEBHOOK_PULIC_HOST` | _(none)_ | Public-facing URL of this service. When set, enables automatic workflow creation for custom fields on startup |
| `max_ctx` | `PAPERLESS_LLM_MAX_CTX` | `0` | Maximum LLM context window in tokens. `0` uses the model's trained maximum. Reduce this to limit memory usage — must be large enough for your largest document plus LLM output |

## Model Options

| Option | Env Variable | Default | Description |
|---|---|---|---|
| `model` | `GGUF_MODEL_PATH` | _(see Required)_ | Path to the GGUF model file |
| `num_gpu_layers` | `NUM_GPU_LAYERS` | `1024` | Number of layers to offload to GPU. `0` means unlimited (offload all layers). Most models have ~48-64 layers, so `1024` effectively means full GPU offload |

## Status Tag Options

paperless-llm-workflows uses tags to communicate document processing state. On first connection it will create these tags if they don't already exist.

| Option | Env Variable | Default | Description |
|---|---|---|---|
| `processing_tag` | `PROCESSING_TAG_NAME` | `🧠 processing` | Display name of tag applied to documents during processing |
| `processing_color` | `PROCESSING_TAG_COLOR` | `#ffe000` | Hex color of the processing tag |
| `finished_tag` | `FINISHED_TAG_NAME` | `🏷️ finished` | Display name of tag applied after successful processing |
| `finished_color` | `FINSHED_TAG_COLOR` | `#40aebf` | Hex color of the finished tag |
| `error_tag_enable` | `ERROR_TAG_ENABLE` | `false` | Whether to apply an error tag when processing fails (opt-in feature) |
| `error_tag` | `ERROR_TAG_NAME` | `⚠️ error` | Display name of the error tag |
| `error_color` | `ERROR_TAG_COLOR` | `#e45858` | Hex color of the error tag |
| `tag_user_name` | `PAPERLESS_USER` | `user` | Username of the paperless user that will be assigned as creator of the status tags |

## Example TOML Configuration

```toml
paperless_server = "https://paperless.example.com"
model = "/path/to/model.gguf"

# Optional: enable auto-workflow setup
webhook_public_base_url = "https://llm-workflows.example.com"

# Optional: limit context window
max_ctx = 16384

# Optional: customize tags
processing_tag = "🧠 LLM processing"
processing_color = "#ffe000"
finished_tag = "✅ LLM done"
finished_color = "#40aebf"

# Optional: enable error tagging
error_tag_enable = true
error_tag = "⚠️ LLM error"
error_color = "#e45858"
```

## Notes

- The model loads on first request and unloads when the processing queue is idle, to conserve memory.
