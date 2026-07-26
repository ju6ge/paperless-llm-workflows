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

# Configuration

Configuration of the software is possible via a configuration file at `/etc/paperless-field-extractor/config.toml` or via environment variables. Environment variables can be used to overwrite values from the configuration file.

Apart from configuration an API Token is required to enable communication with the paperless API! This token should be made available via the `PAPERLESS_API_CLIENT_API_TOKEN` environment variable!!!

This file shows the default configuration and explains the options:
``` toml
# corresponding env var `PAPERLESS_WEBHOOK_HOST` listen address of service
host = "0.0.0.0"
# corresponding env var `PAPERLESS_WEBHOOK_PORT` listen port of service
port = 8123
# corresponding env var `PAPERLESS_SERVER`, defines were the paperless instnace is reachable
paperless_server = "https://example-paperless.domain"
# corresponting env var `WEBHOOK_PUBLIC_BASE_URL`, set reachable public ip of this webserver with this information paperless-llm-workflow can auto setup the required workflows for custom field filling
webhook_public_base_url = "http(s)://paperless-llm-workflows.host{:optional-port}
# corresponding env var `GGUF_MODEL_PATH`, defines where the gguf model file is located
model = "/usr/share/paperless-field-extractor/model.gguf"
# corresponding env var `NUM_GPU_LAYERS`, sets llama cpp option num_cpu_layers when initializing the inference backend zero here means unlimited, most models have way less layers ~50 so this should suffice for full offloading to gpu
num_gpu_layers = 1024
# corresponding env var `PAPERLESS_LLM_MAX_CTX`, sets maximum token size for an inference session if, default value of 0 means that the maximum context used while training of the model will be used. This is potentially very big so it is recommended to use
a lower value. It needs to be big enouth to fit the biggest doc from your paperless instance.
max_ctx = 0

# correspondent suggesting enables the language model to process all inbox documents and add extra suggestions to the correspondet value, this is useful if you have a lot of new document that paperless has not trained for matching yet
# the corresponding environment var is `CORRESPONDENT_SUGGEST`
correspondent_suggestions = false

# corresponding env var `PROCESSING_TAG_NAME`, display name of the tag that is show when a document is being processed
processing_tag = "🧠 processing"
# corresponding env var `PROCESSING_TAG_COLOR`, display color of the tag that is show when a document is being processed
processing_color = "#ffe000"
# corresponding env var `FINISHED_TAG_NAME`, display name of the tag that is show when a document has been fully processed
finished_tag = "🏷️ finished"
# corresponding env var `FINISHED_TAG_COLOR`, display color of the tag that is show when a document has been fully processed
finished_color = "#40aebf"
# corresponding env var "ERROR_TAG_ENABLE", control if error tags are used
error_tag_enable = false
# corresponding env var `ERROR_TAG_NAME`, display name of the tag that is shown when a document processing ran into an error
error_tag = "⚠️ error"
# corresponding env var `ERROR_TAG_COLOR`, display color of the tag that is shown when a document processing ran into an error
error_color = "#e45858"
# corresponding env var `PAPERLESS_USER`, default user to use when creating processing and finshed tags on inital connection
tag_user_name = "user"
```

# Setup

If you just want to run this software for your own instance using a containerized approach is recommended. 

## Containerized Approach

The default container is setup to include a model already and with some environment variables should be fully functional:

``` sh
<podman/docker> run -it --rm \
    --device /dev/kfd \ # give graphics device access to the container
    --device /dev/dri \ # give graphics device access to the container
    -p 8123:8123 \
    -e PAPERLESS_LLM_MAX_CTX=16384 \ # maximum context length of an inference session, needs to be big enought for document + llm output
    -e PAPERLESS_API_CLIENT_API_TOKEN=<token> \
    -e PAPERLESS_SERVER=<paperless_ngx_url> \
    -e PAPERLESS_USER=<user> \ # used for tag creation
    ghcr.io/ju6ge/paperless-llm-workflows:<version>-<backend> server
```

Currently, only the `vulkan` backend has a prebuilt container available, it should be fine for most deployments even without a graphics processor available.


## Building the Container yourself

You can also build the container locally if you prefer. For this the following command will do the trick:

``` sh
<podman/docker> build \
    -f distribution/docker/Dockerfile \
    -t localhost/paperless-llm-workflows:vulkan \
    --build-arg INFERENCE_BACKEND=<backend> \  #this argument is required to select the compute backend, cuda is currenly not supported by the docker build 
    --build-arg MODEL_URL=<url> \  #optionaly you can point the build process to include a different gguf model by providing a download url
    --build-arg MODEL_LICENSE_URL=<url> \  #if you change the model, consider including its license in the container build 
    .
```

# Build from Source

For development or advanced users manual compilation and setup may be desired.

Successfully building requires selecting a compute backend via feature flag:

``` sh
cargo build --release -F <backend>
```

You can select from the following backends:
- cuda (dedicated GPU - nvidia only)
- vulkan (CPU integrated GPU + dedicated GPU)
- rocm (dedicated GPU - amd only + Ryzen AI Max Processors)
- openmp (CPU)

Depending on your selection you will need to have the corresponding system libraries installed on your device, with development headers included.

Afterward building you can setup a config file at `/etc/paperless-field-extractor/config.toml` and run the software. 
You will need to download a model gguf yourself and configure the `GGUF_MODEL_PATH` environment variable or `model` config option to point to its location!

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
