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

# Configuration

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

# Deployment

For detailed deployment instructions (Docker run, docker-compose, building custom containers, bare metal, GPU setup) see the [Deployment Guide](docs/deployment.md).

# Benchmarking

This project includes a comprehensive benchmarking system to evaluate and compare language models for document processing tasks. The benchmarking feature serves two main purposes:

1. **Model Comparison**: Evaluate and compare different models based on their success rates for common document processing tasks
2. **Code Quality Assurance**: Provide a measurable way to assess whether code changes improve or degrade model performance across different models

Benchmarking code paths are not shipped with the container, instead local building of the code with feature flag `benchmark` is required.

## Benchmarking Feature

The benchmarking system tests models on real documents from your paperless instance, evaluating their performance on:
- Custom field extraction
- Correspondent suggestion
- Decision-making based on document content

Results are displayed in a real-time TUI interface and can are saved to JSON files for further analysis.

## Running Benchmarks

### Prerequisites

To run benchmarks, you need:
- A working paperless-ngx instance
- Documents with verified metadata (tags, custom fields, correspondents)
- Appropriate API token configured

### Single Model Benchmarking

Run benchmarks on a single model:

```sh
./paperless-llm-workflows benchmark \
    --paperless-server https://your-paperless.instance \
    --model /path/to/your/model.gguf \
    --verified-docs-tag "verified" \
    --result-file results.json
```

Options:
- `--verified-docs-tag`: Only benchmark documents with this tag (recommended for reliable results)
- `--sample-doc-size`: Limit to N documents (default: all verified documents)
- `--result-file`: Save results to JSON file
- `--view`: Display previously saved results

### Multi-Model Benchmarking

Compare multiple models in parallel:

```sh
./paperless-llm-workflows multi-benchmark \
    --paperless-server https://your-paperless.instance \
    --model-directory /path/to/models/ \
    --output-directory /path/to/results/ \
    --verified-docs-tag "verified" \
    --jobs 4
```

This will:
1. Find all `.gguf` files in the model directory
2. Run benchmarks on each model in parallel (up to `--jobs` concurrent)
3. Save individual results for each model
4. Display a summary comparison

## Benchmarking Results

### Viewing Results

Results are displayed in a real-time TUI interface showing:
- Progress bars for each model
- Overall progress across all models
- Success/failure/error counts
- Success rates by benchmark type

To view saved results:

```sh
./paperless-llm-workflows benchmark --view --result-file results.json
```

### Result Format

Results are saved in JSON format with:
- Model name
- Document ID
- Expected result
- Actual benchmark result
- Success/failure status
- Error messages (if any)

### Interpreting Results

The benchmark tracks three metrics:
- **Success**: Model produced the correct answer
- **Failed**: Model produced an incorrect answer
- **Errored**: Model encountered an error during processing

Success rates are calculated per benchmark type:
- Custom field extraction
- Correspondent suggestion
- Decision making (valid correspondent)
- Decision making (invalid correspondent)

## Benchmarking Workflows

The benchmarking system evaluates models on three types of tasks:

### 1. Custom Field Extraction
Tests the model's ability to extract and fill custom fields from document content. The model must match the exact value that was previously verified for the document.

### 2. Correspondent Suggestion
Tests the model's ability to suggest the correct correspondent (author/sender) based on document content. The model's suggestion is compared against the verified correspondent.

### 3. Decision Making
Tests the model's reasoning capabilities with true/false questions:
- "Is [verified correspondent] the author/sender of this document?" (should answer true)
- "Is [random other correspondent] the author/sender of this document?" (should answer false)

These benchmarks help ensure models can reliably process documents and make correct decisions based on their content.

# Future Work

Depending on interesent and request the following future updates may come:
- Automated Finetuning using LoRa on existing corpus of documents

# LICENSE

This software is licensed under the AGPL-3.0
