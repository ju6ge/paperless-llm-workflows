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
As a base model this software is using a quantized version of `Ministral 3` to reduce the resource requirements and enable running this even with limited resources.

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
    ghcr.io/ju6ge/paperless-llm-workflows:<version>-<backend>
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

# Future Work

Depending on interesent and request the following future updates may come:
- Automated Finetuning using LoRa on existing corpus of documents

# LICENSE

This software is licensed under the AGPL-3.0
