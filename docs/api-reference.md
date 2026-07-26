# API Reference

Browse the interactive API documentation at `http://{server}:8123/api/` after starting the service, or view the [static ReDoc preview](https://redocly.github.io/redoc/?url=https://raw.githubusercontent.com/ju6ge/paperless-llm-workflows/refs/heads/master/openapi.json).

All endpoints accept POST requests with JSON bodies. The `document_url` field is always required and should be the full API URL of the document in your paperless-ngx instance (e.g., `https://paperless.example.com/api/documents/42/`).

## Endpoints

### POST /fill/custom_fields

Auto-fill all empty custom fields on a document.

**Request body**:
```json
{
  "document_url": "https://paperless.example.com/api/documents/42/",
  "ignore_custom_fields": [3, "InvoiceNumber"],
  "next_tag": "fields-filled"
}
```

| Field | Type | Required | Description |
|---|---|---|---|
| `document_url` | string | Yes | paperless API URL of the document |
| `ignore_custom_fields` | array | No | Custom field IDs (int) or names (string) to skip |
| `next_tag` | string | No | Tag name or ID to apply after processing |

---

### POST /fill/target_custom_field

Fill a single custom field by ID.

**Request body**:
```json
{
  "document_url": "https://paperless.example.com/api/documents/42/",
  "custom_field_id": 5,
  "prompt": "Extract the contract expiry date.",
  "next_tag": "updated"
}
```

| Field | Type | Required | Description |
|---|---|---|---|
| `document_url` | string | Yes | paperless API URL of the document |
| `custom_field_id` | integer | Yes | ID of the custom field to fill |
| `prompt` | string | No | Additional context for the LLM |
| `longtext_schema` | object | No | JSON schema for large text fields |
| `next_tag` | string | No | Tag name or ID to apply after processing |

---

### POST /suggest/correspondent

Suggest a correspondent using LLM reasoning.

**Request body**:
```json
{
  "document_url": "https://paperless.example.com/api/documents/42/",
  "next_tag": "correspondent-set"
}
```

| Field | Type | Required | Description |
|---|---|---|---|
| `document_url` | string | Yes | paperless API URL of the document |
| `next_tag` | string | No | Tag name or ID to apply after processing |

---

### POST /suggest/title

Generate a document title.

**Request body**:
```json
{
  "document_url": "https://paperless.example.com/api/documents/42/",
  "template": "{{correspondent}} - {{date}} - {{subject}}",
  "next_tag": "titled"
}
```

| Field | Type | Required | Description |
|---|---|---|---|
| `document_url` | string | Yes | paperless API URL of the document |
| `template` | string | No | Jinja-style title template |
| `next_tag` | string | No | Tag name or ID to apply after processing |

---

### POST /decision

Ask a yes/no question about the document and conditionally assign tags.

**Request body**:
```json
{
  "document_url": "https://paperless.example.com/api/documents/42/",
  "question": "Does this document contain a payment request?",
  "true_tag": "payment-request",
  "false_tag": "informational"
}
```

| Field | Type | Required | Description |
|---|---|---|---|
| `document_url` | string | Yes | paperless API URL of the document |
| `question` | string | Yes | Yes/no question about the document |
| `true_tag` | string | No* | Tag to apply if answer is yes |
| `false_tag` | string | No* | Tag to apply if answer is no |

> At least one of `true_tag` or `false_tag` must be provided.

---

## Common Request Field

| Field | Type | Description |
|---|---|---|
| `next_tag` | string (optional) | Accepts a tag name (string) or tag ID (integer as string). Applied to the document after processing completes, replacing the default `finished` tag. |

## Responses

All endpoints return `202 Accepted` on success. Processing is asynchronous — the document is queued and results are applied after LLM inference completes.

## Error Codes

| Status | Error | Description |
|---|---|---|
| 400 | `DocumentDoesNotExist` | The document ID from `document_url` was not found |
| 400 | `InvalidDocumentId` | The `document_url` does not contain a valid integer document ID |
| 400 | `DocumentUrlParsingIDFailed` | Could not parse a document ID from the `document_url` path |
| 400 | `TagNotFound` | A referenced tag (`true_tag`, `false_tag`, `next_tag`) doesn't exist |
| 400 | `RequestWithoutEffect` | No tags specified for `/decision` — the result would have no effect |
| 401 | `ReceivedRequestFromUnconfiguredServer` | The `document_url` host doesn't match the configured `PAPERLESS_SERVER` |
