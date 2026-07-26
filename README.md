# paperless-llm-workflows

[![License: AGPL-3.0](https://img.shields.io/badge/License-AGPL--3.0-blue.svg)](https://opensource.org/licenses/AGPL-3.0)
[![Rust Build](https://github.com/ju6ge/paperless-llm-workflows/actions/workflows/rust.yml/badge.svg)](https://github.com/ju6ge/paperless-llm-workflows/actions/workflows/rust.yml)
[![Latest Release](https://img.shields.io/github/v/release/ju6ge/paperless-llm-workflows)](https://github.com/ju6ge/paperless-llm-workflows/releases)

A privacy-first, local LLM extension for [paperless-ngx](https://github.com/paperless-ngx/paperless-ngx) that automates document processing through webhook-driven workflows — no cloud providers, no external APIs.

## What It Does

paperless-llm-workflows integrates a local LLM into your paperless-ngx instance as a set of automated workflow steps. Connect it via webhooks in the paperless workflow UI and the LLM will auto-fill custom fields, suggest correspondents, generate document titles, and make conditional decisions on your documents — all processed locally on your hardware.

## Why Not the Alternatives?

This project is **not** a chat interface for your documents and does **not** send anything to external APIs. If you're looking for cloud-based or chat-oriented solutions, consider:

- [paperless-gpt](https://github.com/icereed/paperless-gpt)
- [paperless-ai](https://github.com/clusterzx/paperless-ai)

## Technical Details

- **Inference engine**: [llama.cpp](https://github.com/ggerganov/llama.cpp) — fully local, zero external API calls
- **Default model**: Gemma4 E4B (Q3_0 quantized) — selected for best accuracy-to-resource ratio [blog post, describing method](https://www.felixrichter.tech/posts/llm-benchmarking/)
- **Acceleration backends**: `vulkan`, `cuda`, `rocm`, `openmp` (CPU) — choose one at compile time
- **Runtime**: model loads on first request, unloads after queue is idle to save memory

## Endpoints

| Endpoint | Description |
|---|---|
| `POST /fill/custom_fields` | Auto-fill all empty custom fields on a document |
| `POST /fill/target_custom_field` | Fill a specific custom field by ID (supports custom prompts & JSON schema for longtext) |
| `POST /suggest/correspondent` | Use LLM reasoning to suggest the correct correspondent |
| `POST /suggest/title` | Generate a document title (supports Jinja-style templates) |
| `POST /decision` | Ask a yes/no question about the document and conditionally assign tags |

Browse the full API documentation at `http://{server}:8123/api/` after starting the service, or view the [static preview online](https://redocly.github.io/redoc/?url=https://raw.githubusercontent.com/ju6ge/paperless-llm-workflows/refs/heads/master/openapi.json).

## How It Works

Each endpoint is triggered from a paperless-ngx workflow via webhook. When a webhook fires, the document is placed in a processing queue, gets a `processing` tag, and is sent through the LLM. After completion, results are written back to paperless and the tag is swapped to `finished` (or a custom `next_tag`).

![Workflow Sequence](./workflow_api_sequence.svg)

See the [Workflow Guide](docs/workflow-guide.md) for step-by-step setup instructions.

## Quick Start

Add to your `docker-compose.yml` alongside paperless-ngx:

```yaml
services:
  paperless-llm-workflows:
    image: ghcr.io/ju6ge/paperless-llm-workflows:latest-vulkan
    restart: unless-stopped
    ports:
      - "8123:8123"
    environment:
      - PAPERLESS_SERVER=https://your-paperless.domain
      - PAPERLESS_API_CLIENT_API_TOKEN=your-token-here
      - PAPERLESS_USER=admin
      - PAPERLESS_LLM_MAX_CTX=16384
    devices:
      - /dev/dri
    # For AMD GPUs with KFD:
    # - /dev/kfd
```

Then create webhook workflows in paperless-ngx pointing to `http://paperless-llm-workflows:8123/{endpoint}`.

For full deployment options (GPU variants, custom containers, bare metal) see the [Deployment Guide](docs/deployment.md).

## Supported Custom Field Types

| Type | Supported | Notes |
|---|---|---|
| Boolean | ✅ | Yes/no fields extracted from document content |
| Date | ✅ | Parses dates with format guidance |
| Integer | ✅ | Numeric whole numbers |
| Number | ✅ | Decimal numbers |
| Monetary | ✅ | Currency values (2 decimal places) |
| Text | ✅ | Up to 128 characters |
| Select | ✅ | Chooses from predefined options |
| LargeText | ✅ | Long-form text; supports JSON schema for structured output |
| Document Link | ❌ | Requires cross-document resolution |
| URL | ❌ | Not yet implemented |

## Configuration

Configuration is applied in layered priority (lowest to highest): TOML config file at `/etc/paperless-field-extractor/config.toml`, environment variables, then CLI flags.

| Option | Env Variable | Default | Description |
|---|---|---|---|
| `host` | `PAPERLESS_WEBHOOK_HOST` | `0.0.0.0` | Listen address |
| `port` | `PAPERLESS_WEBHOOK_PORT` | `8123` | Listen port |
| `paperless_server` | `PAPERLESS_SERVER` | — | Your paperless-ngx URL (required) |
| `webhook_public_base_url` | `WEBHOOK_PULIC_HOST` | — | Public URL for auto-setup of workflows |
| `model` | `GGUF_MODEL_PATH` | — | Path to GGUF model file (required) |
| `num_gpu_layers` | `NUM_GPU_LAYERS` | `1024` | GPU layers for offloading (0 = all) |
| `max_ctx` | `PAPERLESS_LLM_MAX_CTX` | `0` | Max context tokens (0 = model default) |
| `processing_tag` | `PROCESSING_TAG_NAME` | `🧠 processing` | Tag applied during processing |
| `processing_color` | `PROCESSING_TAG_COLOR` | `#ffe000` | Processing tag color |
| `finished_tag` | `FINISHED_TAG_NAME` | `🏷️ finished` | Tag applied after successful processing |
| `finished_color` | `FINSHED_TAG_COLOR` | `#40aebf` | Finished tag color |
| `error_tag_enable` | `ERROR_TAG_ENABLE` | `false` | Enable error tagging (opt-in) |
| `error_tag` | `ERROR_TAG_NAME` | `⚠️ error` | Error tag name |
| `error_color` | `ERROR_TAG_COLOR` | `#e45858` | Error tag color |
| `tag_user_name` | `PAPERLESS_USER` | `user` | Paperless username for tag creation |

The `PAPERLESS_API_CLIENT_API_TOKEN` environment variable (not a config option) is also required — it authenticates with the paperless-ngx API.

For detailed configuration examples and advanced setups, see the [Configuration Reference](docs/configuration.md).

## Additional Resources

- [API Reference](docs/api-reference.md) — Endpoints, request/response schemas, error codes
- [Benchmarking](docs/benchmarking.md) — Evaluate and compare LLM models against your documents
- [Building from Source](docs/building-from-source.md) — Compile with custom backends and features
- [Configuration Reference](docs/configuration.md) — Full option reference with defaults
- [Deployment Guide](docs/deployment.md) — Docker, compose, bare metal, GPU setup
- [FAQ & Troubleshooting](docs/faq-troubleshooting.md) — Common issues and solutions
- [Workflow Guide](docs/workflow-guide.md) — Step-by-step webhook setup in paperless-ngx

## License

AGPL-3.0
