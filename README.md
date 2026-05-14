paperless-llm-workflows
=========================

This project is an extension to the excellent [paperless-ngx](https://github.com/paperless-ngx/paperless-ngx) software.

# Who is this project for?

This project is for people who want to expand the machine learning capabilities of paperless without sacrificing privacy. The 
goal is to integrate language model capabilities into paperless seamlessly without yet another chat and prompting interface or 
complex user interface. This is a standalone project and does not require an extra service to provide model inference everything is baked in already.

If you are looking for document chat or don't care and are fine sending all your private documents to the big cloud providers checkout these projects:
- [paperless-gpt](https://github.com/icereed/paperless-gpt)
- [paperless-ai](https://github.com/clusterzx/paperless-ai)

## Under the Hood

Under the hood this software is running `llama.cpp` as an inference engine to provide a local language model without depending on any cloud providers. Depending on the selected feature it is possible to run
with `cuda`, `vulkan` or `openmp` acceleration.
As a base model this software is using a quantized version of `Qwen3 4B` to reduce the resource requirements and enable running this even with limited resources.
If you are interested in how I selected this model, you can read my [blog post](https://www.felixrichter.tech/posts/llm-benchmarking/) about the process ;).

Long term I want expand the features to enable fine tuning models to your document corpus. This is where the actual learning would come in.

# Usage

This project spawns an API server that can be integrated to provide custom processing steps via `paperless` Workflow feature.

## LLM Workflows

After starting the service you can navigate to `http://{paperless-llm-workflows.ip}:8123/api/` to get an up to date API documentation describing all the endpoints.

If you wish to inspect the documentation online here is a [preview link](https://redocly.github.io/redoc/?url=https://raw.githubusercontent.com/ju6ge/paperless-field-extractor/refs/heads/master/openapi.json).

To integrate a functionality into paperless you need add it as webhook trigger in your paperless worflows:

![Paperless Webhook](./example-workflow-action.png)

When a webhook gets triggered the document will be added to the processing queue of `paperless-llm-workflows` with the corresponding action. The document will be given a 
`processing` tag to make paperless users aware that the document still has pending updates. Once all processing requests for a document have been completed the document will 
be updated with the results and is given a `finished` tag. If you wish to assign a specific tag on process completion there is an extra parameter too the webhook which you 
can use to overwrite what tag will be assigned once the processing step has completed. This process is shown in the following sequence diagram.

![LLM Workflow Sequence](./workflow_api_sequence.svg)

As of now the following llm workflows are availible:
- `/fill/custom_fields`: For all supported custom fields datatypes extract value from document content
- `/suggest/correspondent`: Suggest document correspondent by using reasoning
- `/decision`: Ask a true or false question about the document and set tags depending on result


# Configuration

Configuration of the software is possible via a configuration file at `/etc/paperless-field-extractor/config.toml` or via environment variables. Environment variables can be used to overwrite values from the configuration file.

Apart from configuration an API Token is required to enable communication with the paperless API! This token should be made availible via the `PAPERLESS_API_CLIENT_API_TOKEN` environment variable!!!

This file shows the default configuration and explains the options:
``` toml
# corresponding env var `PAPERLESS_WEBHOOK_HOST` listen address of service
host = "0.0.0.0"
# corresponding env var `PAPERLESS_WEBHOOK_PORT` listen port of service
port = 8123
# corresponding env var `PAPERLESS_SERVER`, defines were the paperless instnace is reachable
paperless_server = "https://example-paperless.domain"
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
    -p 8123:8123
    -e PAPERLESS_LLM_MAX_CTX=16384 \ # maximum context length of an inference session, needs to be big enought for document + llm output
    -e PAPERLESS_API_CLIENT_API_TOKEN=<token> \
    -e PAPERLESS_SERVER=<paperless_ngx_url> \
    -e PAPERLESS_USER=<user> \ # used for tag creation
    ghcr.io/ju6ge/paperless-llm-workflows:<version>-<backend> server
```

Currently only the `vulkan` backend has a prebuilt container availible, it should be fine for most deployments even without a graphics processor availible.


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

Successfull building requires selecting a compute backend via feature flag:

``` sh
cargo build --release -F <backend>
```

You can select from the following backends:
- native (CPU)
- openmp (CPU)
- cuda (GPU)
- vulkan (CPU + GPU)

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
