# FAQ & Troubleshooting

## General

### What hardware do I need?

The minimum is a CPU with enough RAM to load the model (~2 GB for Qwen3 4B Q4_0). A GPU (AMD, Intel, or NVIDIA) via Vulkan dramatically improves processing speed. The `openmp` backend works on any system without GPU acceleration.

### How much memory does it use?

The Qwen3 4B Q4_0 model requires about 2 GB of RAM when loaded. Peak memory usage depends on `max_ctx` — larger context windows need more memory. The model unloads automatically when the processing queue is idle.

### Can I run this without a GPU?

Yes — build or run with the `openmp` backend for CPU-only inference. Expect slower processing (several seconds per document instead of milliseconds).

## Setup & Deployment

### The container starts but documents don't get processed

Check the following:
1. Verify `PAPERLESS_SERVER` matches the URL paperless-ngx uses in webhook calls (not necessarily the public URL)
2. Verify `PAPERLESS_API_CLIENT_API_TOKEN` has document read/write permissions
3. Check container logs for errors: `docker logs paperless-llm-workflows`
4. Confirm paperless-ngx can reach the `paperless-llm-workflows` service (check network connectivity)

### "Received request from unconfigured server" error

The `document_url` in the webhook request has a different host than your configured `PAPERLESS_SERVER`. This is a security check. If you're running both in Docker, set `PAPERLESS_SERVER` to the internal container URL (e.g., `http://paperless:8000`), not the public address.

### The model takes too long to load on first request

This is expected — the model loads from disk on the first document request. Subsequent requests while the model is loaded are fast. The model stays loaded until the processing queue is idle, then unloads to free memory.

### How do I increase processing speed?

1. Use a GPU with Vulkan acceleration (the `vulkan` backend)
2. Set `NUM_GPU_LAYERS=1024` or `0` to offload all layers to GPU
3. Increase `PAPERLESS_LLM_MAX_CTX` only as much as needed — larger context windows slow down inference
4. Consider a faster model (e.g., Qwen3 8B) if your hardware supports it

## Webhooks & Workflows

### "Tag not found" error

The `true_tag`, `false_tag`, or `next_tag` references a tag that doesn't exist in paperless. Create the tag first, or use an existing tag name/ID.

### Can I chain multiple workflows?

Yes — use the `next_tag` parameter on each endpoint. Set `next_tag` to a tag that triggers your next workflow step. See the [Workflow Guide](workflow-guide.md) for examples.

### My webhook works manually but not from paperless workflows

Make sure paperless-ngx can reach the service. If both run in Docker, they need to share a network. The `document_url` sent by paperless should resolve from within the `paperless-llm-workflows` container.

### Processing tag is never removed

If the service crashes during processing, the `processing` tag may persist. Check:
1. Service logs for errors
2. Whether the LLM model loaded successfully
3. Whether `PAPERLESS_API_CLIENT_API_TOKEN` has sufficient permissions to update documents

## Custom Fields

### Why isn't a custom field being filled?

Check:
1. The custom field type is [supported](../README.md#supported-custom-field-types) — Document Link and URL are not yet supported
2. The field is actually empty — only empty fields are filled by `/fill/custom_fields`
3. The LLM model has enough context to extract the value (check `max_ctx`)
4. For LargeText fields, consider providing a `longtext_schema` via `/fill/target_custom_field`

### Text fields are truncated to 128 characters

This is intentional — text custom field values are limited to 128 characters. Use the LargeText type for longer content.

### Dates aren't extracted correctly

The LLM extracts dates based on format guidance built into the prompt. If your documents use an unusual date format, try using `/fill/target_custom_field` with a custom `prompt` to guide extraction.

## Tag Configuration

### How do I change the processing/finished tags?

Set `PROCESSING_TAG_NAME`, `FINISHED_TAG_NAME`, `PROCESSING_TAG_COLOR`, and `FINISHED_TAG_COLOR` environment variables (or the corresponding TOML config options). On first startup, the service creates these tags if they don't exist.

### Can I disable the error tag?

Error tagging is opt-in. Keep `ERROR_TAG_ENABLE=false` (the default) to disable it. When enabled, the error tag is applied to documents that fail during LLM processing.
