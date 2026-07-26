# API Overview

The full, auto-generated API documentation is available at `http://{server}:8123/api/` after starting the service (powered by the [OpenAPI spec](openapi.json)). View the [static ReDoc preview](https://redocly.github.io/redoc/?url=https://raw.githubusercontent.com/ju6ge/paperless-llm-workflows/refs/heads/master/openapi.json) online.

This page covers how the API works conceptually — the processing model, request contracts, and error handling.

## Async Processing Model

Every endpoint is **asynchronous**. When you send a POST request:

1. The request is validated and the document is queued for processing
2. The service responds with `202 Accepted` immediately
3. The document receives a `processing` tag
4. The LLM processes the document in the background
5. Results are written back to paperless-ngx via its API
6. The tag is swapped to `finished` (or the custom `next_tag`)

You do not need to poll for results — the document is updated directly in paperless-ngx.

## The `document_url` Contract

Every request must include a `document_url` field containing the full API URL of the document in your paperless-ngx instance, for example:

```
https://paperless.example.com/api/documents/42/
```

The host portion of this URL **must** match the `PAPERLESS_SERVER` configuration value. This is a security check to prevent external servers from triggering workflows against your instance. Mismatches return `401 Unauthorized`.

The document ID is extracted from the URL path — malformed URLs will be rejected.

## The `next_tag` Parameter

All endpoints accept an optional `next_tag` field. When set, the specified tag (by name or numeric ID) is applied to the document after processing completes, instead of the default `finished` tag. This enables chaining multiple workflow steps:

```json
{
  "document_url": "https://paperless.example.com/api/documents/42/",
  "next_tag": "needs-review"
}
```

After LLM processing, the document receives the `needs-review` tag, which can trigger a second workflow in paperless-ngx.

## Error Handling

| Status | Meaning |
|---|---|
| `400` | Bad request — missing required fields, invalid document URL, unknown tag, or decision request without any tags |
| `401` | Security check failed — the `document_url` host does not match the configured `PAPERLESS_SERVER` |

Error responses include a descriptive error name (e.g., `DocumentDoesNotExist`, `TagNotFound`, `RequestWithoutEffect`) in the response body.

## Request Format

All endpoints accept POST with `application/json` content type. The request body is a JSON object with endpoint-specific parameters plus the universal `document_url` and optional `next_tag` fields.

## Available Endpoints

| Endpoint | Purpose |
|---|---|
| `POST /fill/custom_fields` | Auto-fill all empty custom fields on a document |
| `POST /fill/target_custom_field` | Fill a specific custom field by ID |
| `POST /suggest/correspondent` | Suggest the correct correspondent via LLM reasoning |
| `POST /suggest/title` | Generate a document title (with optional template) |
| `POST /decision` | Ask a yes/no question and conditionally assign tags |

For parameter details, request schemas, and examples per endpoint, refer to the [auto-generated API documentation](http://{server}:8123/api/).
