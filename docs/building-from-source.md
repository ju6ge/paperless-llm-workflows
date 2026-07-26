# Building from Source

## Prerequisites

- Rust toolchain (stable, 2024 edition)
- `cargo` available on PATH
- System libraries for your chosen compute backend

## Selecting a Backend

You must choose exactly one compute backend via Cargo feature flags:

```bash
cargo build --release -F <backend>
```

| Backend | Feature Flag | Hardware | Notes |
|---|---|---|---|
| Vulkan | `vulkan` | AMD, Intel, NVIDIA GPU + CPU fallback | Most portable GPU option |
| CUDA | `cuda` | NVIDIA GPU only | Requires CUDA toolkit |
| ROCm | `rocm` | AMD GPU, Ryzen AI Max | Requires ROCm libraries |
| OpenMP | `openmp` | Any CPU | No GPU required |

> You can only select **one** backend. Attempting to enable multiple backends will produce a compile error.

## System Dependencies

### Vulkan

```bash
# Debian/Ubuntu
sudo apt install build-essential libclang-dev cmake libvulkan-dev libshaderc-dev glslc

# Arch Linux
sudo pacman -S base-devel clang cmake vulkan-icd-loader
# Plus your GPU's Vulkan driver:
sudo pacman -S vulkan-radeon   # AMD
sudo pacman -S vulkan-intel    # Intel
```

### CUDA

```bash
# Install NVIDIA CUDA toolkit
# Then ensure include and lib paths are set
export CPATH=/usr/local/cuda/include
export LIBRARY_PATH=/usr/local/cuda/lib64
```

### ROCm

```bash
# Install AMD ROCm runtime
# Ensure rocblas and hipBLAS are available
```

### OpenMP (CPU)

```bash
# Debian/Ubuntu
sudo apt install build-essential libclang-dev cmake
```

## Building

```bash
# Release build with Vulkan
cargo build --release -F vulkan

# Release build with benchmarking support
cargo build --release -F vulkan -F benchmark
```

## Running

After building, the binary is at `target/release/paperless-llm-workflows`:

```bash
./target/release/paperless-llm-workflows server \
    --paperless-server https://paperless.example.com \
    --model /path/to/model.gguf
```

Or set required values via environment:

```bash
export PAPERLESS_SERVER=https://paperless.example.com
export PAPERLESS_API_CLIENT_API_TOKEN=your-token
export GGUF_MODEL_PATH=/path/to/model.gguf
export PAPERLESS_USER=admin

./target/release/paperless-llm-workflows server
```

## Getting a Model

Download a GGUF model from HuggingFace:

```bash
mkdir -p /opt/models
curl -Lo /opt/models/qwen3-4b.gguf \
    "https://huggingface.co/unsloth/Qwen3-4B-GGUF/resolve/main/Qwen3-4B-Q4_0.gguf?download=true"
```

Then point `GGUF_MODEL_PATH` (or `--model`) to the downloaded file.

## Development Build

For iterative development, use debug builds (slower but faster to compile):

```bash
cargo build -F vulkan
```

Run with `RUST_LOG=debug` for verbose logging:

```bash
RUST_LOG=debug ./target/debug/paperless-llm-workflows server --paperless-server ...
```
