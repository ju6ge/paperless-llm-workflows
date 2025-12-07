ARG INFERENCE_BACKEND="vulkan"
# using quantized version of qwen3 8b for more resource efficiency
ARG MODEL_URL="https://huggingface.co/robolamp/Ministral-3-8B-Base-2512-GGUF/resolve/main/Ministral-3-8B-Base-2512-Q2_K.gguf?download=true"
ARG MODEL_LICENSE_URL="https://www.apache.org/licenses/LICENSE-2.0.txt"

FROM docker.io/rust:latest as builder
ARG INFERENCE_BACKEND

WORKDIR /build

COPY Cargo.toml Cargo.lock ./
COPY src ./src

RUN apt-get update && apt-get install -y --no-install-recommends \
    build-essential \
    libclang-dev \
    cmake && \
    if [ "${INFERENCE_BACKEND}" = "vulkan" ]; then apt install libvulkan-dev libshaderc-dev glslc -y; fi
    

RUN cargo build --release -F "${INFERENCE_BACKEND}"

FROM docker.io/debian:trixie-slim
ARG INFERENCE_BACKEND
ARG MODEL_URL
ARG MODEL_LICENSE_URL
ENV GGUF_MODEL_PATH="/srv/model/model.gguf"

RUN apt-get update && \
    apt-get install -y curl && \
    if [ "${INFERENCE_BACKEND}" = "vulkan" ]; then apt-get install libvulkan-dev libshaderc-dev glslc -y; fi

RUN mkdir -p $(dirname "$GGUF_MODEL_PATH") && curl -Lo "$GGUF_MODEL_PATH" "$MODEL_URL" && curl -Lo "$(dirname "$GGUF_MODEL_PATH")/LICENSE" "$MODEL_LICENSE_URL"

COPY --from=builder /build/target/release/paperless-llm-workflows /usr/local/bin/

ENTRYPOINT [ "/usr/local/bin/paperless-llm-workflows" ]
