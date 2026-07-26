# Deployment Guide

## Prebuilt Container (Recommended)

The easiest way to run paperless-llm-workflows is with the prebuilt container from GitHub Container Registry. Only the `vulkan` backend has a prebuilt image.

### Docker Run

```bash
docker run -d \
    --name paperless-llm-workflows \
    --restart unless-stopped \
    --device /dev/dri \
    --network paperless-network \
    -p 8123:8123 \
    -e PAPERLESS_SERVER=https://your-paperless.domain \
    -e PAPERLESS_API_CLIENT_API_TOKEN=your-token \
    -e PAPERLESS_USER=admin \
    -e PAPERLESS_LLM_MAX_CTX=16384 \
    ghcr.io/ju6ge/paperless-llm-workflows:latest-vulkan \
    server
```

**Devices**:
- `/dev/dri` — AMD/NVIDIA/Intel GPU via Vulkan (required for GPU acceleration)
- `/dev/kfd` — AMD GPU compute (optional, for ROCm-compatible Vulkan)

### Docker Compose

Add to your existing `docker-compose.yml` alongside paperless-ngx:

```yaml
services:
  paperless-llm-workflows:
    image: ghcr.io/ju6ge/paperless-llm-workflows:latest-vulkan
    container_name: paperless-llm-workflows
    restart: unless-stopped
    ports:
      - "8123:8123"
    environment:
      - PAPERLESS_SERVER=http://paperless:8000
      - PAPERLESS_API_CLIENT_API_TOKEN=your-token
      - PAPERLESS_USER=admin
      - PAPERLESS_LLM_MAX_CTX=16384
    devices:
      - /dev/dri
      # - /dev/kfd  # AMD GPUs
    networks:
      - paperless-net

networks:
  paperless-net:
    external: true
    # Or: driver: bridge (if creating a new network)
```

> **Note**: When running both paperless-ngx and paperless-llm-workflows in compose, set `PAPERLESS_SERVER` to the paperless container's internal URL (e.g., `http://paperless:8000`), not the public URL.

### Version Tags

| Tag | Description |
|---|---|
| `latest-vulkan` | Latest release, Vulkan backend |
| `v0.4.0-vulkan` | Specific version, Vulkan backend |

---

## Building a Custom Container

Build your own container to select a different backend, model, or to include custom configuration.

### Build with Vulkan Backend

```bash
docker build \
    -f distribution/docker/Dockerfile \
    -t localhost/paperless-llm-workflows:vulkan \
    --build-arg INFERENCE_BACKEND=vulkan \
    .
```

### Available Build Arguments

| Argument | Description | Example |
|---|---|---|
| `INFERENCE_BACKEND` | Compute backend (required) | `vulkan`, `openmp` |
| `MODEL_URL` | URL to download a custom GGUF model | `https://huggingface.co/.../resolve/main/model.gguf` |
| `MODEL_LICENSE_URL` | URL to the model license file | `https://www.apache.org/licenses/LICENSE-2.0.txt` |

### Build with Custom Model

```bash
docker build \
    -f distribution/docker/Dockerfile \
    -t localhost/paperless-llm-workflows:custom \
    --build-arg INFERENCE_BACKEND=vulkan \
    --build-arg MODEL_URL="https://huggingface.co/your-org/model/resolve/main/model.gguf" \
    --build-arg MODEL_LICENSE_URL="https://raw.githubusercontent.com/your-org/model/main/LICENSE" \
    .
```

> **Note**: `cuda` backend is currently not supported by the Docker build. Use the `vulkan` backend for NVIDIA GPUs, or build from source.

---

## Bare Metal / Systemd

For running without containers, build from source and deploy as a system service.

### Build from Source

See [Building from Source](building-from-source.md) for compile instructions.

### Systemd Service

Create `/etc/systemd/system/paperless-llm-workflows.service`:

```ini
[Unit]
Description=paperless-llm-workflows
After=network.target

[Service]
Type=simple
Environment=PAPERLESS_SERVER=https://paperless.example.com
Environment=PAPERLESS_API_CLIENT_API_TOKEN=your-token
Environment=GGUF_MODEL_PATH=/opt/models/qwen3-4b.gguf
Environment=PAPERLESS_LLM_MAX_CTX=16384
ExecStart=/usr/local/bin/paperless-llm-workflows server
Restart=always
RestartSec=10

[Install]
WantedBy=multi-user.target
```

```bash
sudo systemctl enable --now paperless-llm-workflows
```

---

## GPU Setup Notes

### Vulkan (AMD / Intel / NVIDIA)

Install Vulkan development packages before building:

```bash
# Debian/Ubuntu
sudo apt install libvulkan-dev libshaderc-dev glslc

# Arch Linux
sudo pacman -S vulkan-icd-loader vulkan-radeon  # AMD
sudo pacman -S vulkan-icd-loader vulkan-intel    # Intel
sudo pacman -S vulkan-icd-loader vulkan-nouveau   # NVIDIA (open-source)
```

### CUDA (NVIDIA — source builds only)

Build with `cargo build --release -F cuda`. Requires NVIDIA CUDA toolkit and development libraries installed on the host.

### ROCm (AMD — source builds only)

Build with `cargo build --release -F rocm`. Requires AMD ROCm libraries.

### CPU Only

Build with `cargo build --release -F openmp`. No GPU required — runs on any system. Slower but functional.

---

## Model Selection

The default container ships with Qwen3 4B (Q4_0 quantized). To use a different model:

1. Download a GGUF model to your host
2. Mount or copy it into the container at the path specified by `GGUF_MODEL_PATH`
3. Adjust `PAPERLESS_LLM_MAX_CTX` to fit your document sizes

Example mount:
```bash
docker run -v /opt/models:/srv/models \
    -e GGUF_MODEL_PATH=/srv/models/your-model.gguf \
    ghcr.io/ju6ge/paperless-llm-workflows:latest-vulkan \
    server
```

---

## Auto-Workflow Setup

Setting `WEBHOOK_PULIC_HOST` (or `webhook_public_base_url` in config) enables automatic workflow creation on startup. The service will:

1. Discover all custom fields on your paperless instance
2. Create workflows for fields that don't already have one
3. Configure them to call `/fill/target_custom_field` on document consumption

Set this to the public URL where paperless-ngx can reach the service.
