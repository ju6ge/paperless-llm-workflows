use std::{
    collections::{BTreeMap, VecDeque},
    hash::Hash,
    path::Path,
    sync::Arc,
    time::Duration,
};

use actix_web::{
    App, HttpResponse, HttpServer, ResponseError,
    dev::HttpServiceFactory,
    http::{StatusCode, Uri},
    post,
    web::{self, Data},
};
use itertools::Itertools;
use log::error;
use once_cell::sync::Lazy;
use paperless_api_client::{
    Client,
    types::{CustomField, CustomFieldInstance, Document, Tag},
};
use regex::Regex;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use thiserror::Error;
use tokio::{
    join, spawn,
    sync::{Mutex, RwLock},
    task::{JoinError, spawn_blocking},
};
use utoipa::{OpenApi, ToSchema};
use utoipa_swagger_ui::SwaggerUi;

static DOCID_REGEX: Lazy<Regex> = regex_static::lazy_regex!(r"documents/(\d*)");

static PROCESSING_QUEUE: Lazy<tokio::sync::RwLock<VecDeque<DocumentProcessingRequest>>> =
    Lazy::new(|| RwLock::new(VecDeque::new()));

// shutdown bit, when this is set to true the document processing pipeline will be shut down
static STOP_FLAG: Lazy<tokio::sync::RwLock<bool>> = Lazy::new(|| RwLock::new(false));

// model will only be initialized and stored if there are documents that need processing
static MODEL_SINGLETON: Lazy<tokio::sync::Mutex<Option<LLModelExtractor>>> =
    Lazy::new(|| Mutex::new(None));

use crate::{
    config::Config,
    extract::{LLModelExtractor, ModelError},
    requests,
    types::{
        Decision, FieldError, FieldExtract, custom_field_learning_supported,
        schema_from_decision_question,
    },
};

#[derive(Debug, PartialEq, Clone)]
#[non_exhaustive]
enum ProcessingType {
    CustomFieldPrediction {
        exclude_fields: Option<Vec<CustomField>>,
    },
    TargetedCustomField{
        custom_field_id: i64,
        prompt: Option<String>
    },
    CorrespondentSuggest,
    DecsionTagFlow {
        question: String,
        true_tag: Option<Tag>,
        false_tag: Option<Tag>,
    },
    TitleSuggest {
        template: Option<String>,
    },
}

impl Hash for ProcessingType {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        // this is only used to remove duplicate processing types in some operations
        // these do not care about sub parameters, just about the kind of request
        core::mem::discriminant(self).hash(state);
    }
}

impl Eq for ProcessingType {}

/// Most processing of document will involve feeding document data throuh a large languae model.
/// Since LLM are notoriously resource intensive a task queue is used in order to facilitate asyncronous
/// batched processinsg a container is required to hold the queue of processing requests
struct DocumentProcessingRequest {
    document: Document,
    processing_type: ProcessingType,
    overwrite_finshed_tag: Option<Tag>,
}

#[derive(Debug, Error)]
enum DocumentProcessingError {
    #[error("Could not start LLM processinsg thread: {0}")]
    SpawningProcessingThreadFailed(#[from] JoinError),
    #[error(transparent)]
    ModelProcessingError(#[from] ModelError),
    #[error(transparent)]
    PaperlessCommunicationError(#[from] paperless_api_client::types::error::Error),
    #[error(transparent)]
    ExtractionError(#[from] FieldError),
}

async fn handle_correspondend_suggest(
    doc: &mut Document,
    api_client: &mut Client,
) -> Result<(), DocumentProcessingError> {
    let crrspndts = requests::fetch_all_correspondents(api_client).await;
    let crrspndts_suggest_schema = crate::types::schema_from_correspondents(crrspndts.as_slice());

    let doc_data = serde_json::to_value(&doc.content).unwrap();

    let extracted_correspondent = spawn_blocking(move || {
        let mut model_singleton = MODEL_SINGLETON.blocking_lock();
        if let Some(model) = model_singleton.as_mut() {
            model.extract(&doc_data, &crrspndts_suggest_schema, false)
        } else {
            Err(crate::extract::ModelError::ModelNotLoaded)
        }
    })
    .await??;

    log::debug!(
        "Suggested new correspondent for doc {}: \n{extracted_correspondent:#?}",
        doc.id
    );

    let extracted_correspondent: FieldExtract =
        serde_json::from_value(extracted_correspondent).map_err(FieldError::from)?;

    let new_crrspndt = extracted_correspondent.to_correspondent(&crrspndts)?;
    doc.correspondent = Some(new_crrspndt.id);

    // defered sync back to paperless instance
    // after successfull finish the state of document on paperless will be updated by the update task
    Ok(())
}

async fn handle_custom_field_prediction(
    doc: &mut Document,
    ignore_custom_fields: &Option<Vec<CustomField>>,
    api_client: &mut Client,
) -> Result<(), DocumentProcessingError> {
    // fetch all custom field definitions for fields on the document that need to be filled
    let relevant_custom_fields: Vec<CustomField> =
    if let Some(cf_ids) = doc.custom_fields.as_ref().map(|cfis| {
            cfis.iter()
                .filter(|cfi| {
                    ignore_custom_fields
                        .as_ref()
                        .is_none_or(|ref ignored_fields| {
                            ignored_fields.iter().find(|ignored_cf| ignored_cf.id == cfi.field).is_none()
                        })
                })
                .filter(|cfi| cfi.value.is_none())
                .map(|cfi| cfi.field)
                .collect()
        })
    {
        requests::get_custom_fields_by_id(
            api_client,
            cf_ids
        ) .await
    } else {
        vec![]
    }
    // this filters out all custom fields that are currently unsupported
    .into_iter().filter(|cf| {
        let learning_supported = custom_field_learning_supported(cf);
        if !learning_supported {
            log::warn!("Custom Fields with name `{}` are using an unsupported custom field type, will be ignored!", cf.name);
        }
        learning_supported
    }).collect();

    for cf in relevant_custom_fields {
        let doc_data = serde_json::to_value(&doc).unwrap();

        if let Some(field_grammar) = crate::types::schema_from_custom_field(&cf) {
            let extracted_cf = spawn_blocking(move || {
                let mut model_singleton = MODEL_SINGLETON.blocking_lock();
                if let Some(model) = model_singleton.as_mut() {
                    model.extract(&doc_data, &field_grammar, false)
                } else {
                    Err(crate::extract::ModelError::ModelNotLoaded)
                }
            })
            .await??;

            let extracted_cf: FieldExtract =
                serde_json::from_value(extracted_cf).map_err(FieldError::from)?;

            if let Ok(cf_value) = extracted_cf.to_custom_field_instance(&cf).map_err(|err| {
                log::error!("{err}");
                err
            }) {
                // update document custom fields on server side
                // sending the updated document to the server will happen afterwards
                log::debug!(
                    "Extracted custom field for document {}\n {:#?}",
                    doc.id,
                    cf_value
                );
                if let Some(doc_custom_fields) = doc.custom_fields.as_mut() {
                    for doc_cf_i in doc_custom_fields.iter_mut() {
                        if doc_cf_i.field == cf_value.field {
                            *doc_cf_i = cf_value.clone();
                        }
                    }
                }
            }
        }
    }

    // defered sync back to paperless instance
    // after successfull finish the state of document on paperless will be updated by the update task
    Ok(())
}

async fn handle_title_suggestion(
    doc: &mut Document,
    _api_client: &mut Client,
    template: &Option<String>,
) -> Result<(), DocumentProcessingError> {
    let title_schema = crate::types::schema_from_title_template(template.as_deref());

    let doc_data = serde_json::to_value(&doc.content).unwrap();

    let extracted = spawn_blocking(move || {
        let mut model_singleton = MODEL_SINGLETON.blocking_lock();
        if let Some(model) = model_singleton.as_mut() {
            model.extract(&doc_data, &title_schema, false)
        } else {
            Err(crate::extract::ModelError::ModelNotLoaded)
        }
    })
    .await??;

    log::debug!(
        "Extracted title fields for document {}\n{extracted:#?}",
        doc.id
    );

    let title = match template {
        Some(t) if !t.is_empty() => {
            let fields: serde_json::Map<String, serde_json::Value> =
                serde_json::from_value(extracted).map_err(FieldError::from)?;
            let keys = crate::types::parse_title_template(t);
            let mut result = t.clone();
            for key in keys {
                let val = fields.get(&key).and_then(|v| v.as_str()).unwrap_or("");
                result = result.replace(&format!("{{{{{}}}}}", key), val);
            }
            result
        }
        _ => {
            let fields: serde_json::Map<String, serde_json::Value> =
                serde_json::from_value(extracted).map_err(FieldError::from)?;
            fields.get("title")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string()
        }
    };

    doc.title = Some(title);

    Ok(())
}

async fn handle_decision(
    doc: &mut Document,
    question: &String,
    true_tag: Option<&Tag>,
    false_tag: Option<&Tag>,
) -> Result<(), DocumentProcessingError> {
    let decision_schema = schema_from_decision_question(question);

    let doc_data = serde_json::to_value(&doc.content).unwrap();

    let extracted_answer = spawn_blocking(move || {
        let mut model_singleton = MODEL_SINGLETON.blocking_lock();
        if let Some(model) = model_singleton.as_mut() {
            model.extract(&doc_data, &decision_schema, false)
        } else {
            Err(crate::extract::ModelError::ModelNotLoaded)
        }
    })
    .await??;

    let extracted_decision: Decision =
        serde_json::from_value(extracted_answer).map_err(FieldError::from)?;

    log::debug!(
        "Made decision on document {}\n{extracted_decision:#?}",
        doc.id
    );

    if let Some(true_tag) = true_tag
        && extracted_decision.answer_bool
        && !doc.tags.contains(&true_tag.id)
    {
        doc.tags.push(true_tag.id);
    }

    if let Some(false_tag) = false_tag
        && !extracted_decision.answer_bool
        && !doc.tags.contains(&false_tag.id)
    {
        doc.tags.push(false_tag.id);
    }

    Ok(())
}

async fn handle_targeted_custom_field_prediction(
    doc: &mut Document,
    custom_field: CustomField,
    prompt: Option<String>,
) -> Result<(), DocumentProcessingError> {
    let doc_data = serde_json::to_value(&doc).unwrap();

    if let Some(field_grammar) = if let Some(prompt) = prompt {
        crate::types::schema_from_custom_field_with_prompt(&custom_field, prompt)
    } else {
        crate::types::schema_from_custom_field(&custom_field)
    } {
        let extracted_cf = spawn_blocking(move || {
            let mut model_singleton = MODEL_SINGLETON.blocking_lock();
            if let Some(model) = model_singleton.as_mut() {
                model.extract(&doc_data, &field_grammar, false)
            } else {
                Err(crate::extract::ModelError::ModelNotLoaded)
            }
        })
        .await??;

        let extracted_cf: FieldExtract =
            serde_json::from_value(extracted_cf).map_err(FieldError::from)?;

        if let Ok(cf_value) = extracted_cf
            .to_custom_field_instance(&custom_field)
            .map_err(|err| {
                log::error!("{err}");
                err
            })
        {
            // update document custom fields on server side
            // sending the updated document to the server will happen afterwards
            log::debug!(
                "Extracted custom field for document {}\n {:#?}",
                doc.id,
                cf_value
            );
            if let Some(doc_custom_fields) = doc.custom_fields.as_mut() {
                for doc_cf_i in doc_custom_fields.iter_mut() {
                    if doc_cf_i.field == cf_value.field {
                        *doc_cf_i = cf_value.clone();
                    }
                }
            }
        }
    }

    // defered sync back to paperless instance
    // after successfull finish the state of document on paperless will be updated by the update task
    Ok(())
}

#[derive(Debug, thiserror::Error)]
enum WebhookError {
    #[error("Document with id `{0}` does not exist!")]
    DocumentDoesNotExist(i64),
    #[error("Document ID is not a valid integer!")]
    InvalidDocumentId,
    #[error(
        "The provided list of custom fields is invalid, custom fields that should be ignored need to be referenced by their integer id or sting name!"
    )]
    InvalidIgnoreCustomFieldValue,
    #[error("Could not parse Document ID from `document_url` field!")]
    DocumentUrlParsingIDFailed,
    #[error("Document Url points to a server unrelated to this configuration. Ignoring Request")]
    ReceivedRequestFromUnconfiguredServer,
    #[error("Request specified tag {0}, but it could not be found, neither id nor name exists!")]
    TagNotFound(String),
    #[error(
        "The request is configured in a way that nothing is going to happen when the request completes, so it will be ignored!"
    )]
    RequestWithoutEffect,
}

#[derive(Debug, Serialize, Deserialize, ToSchema)]
struct DecisionTagFlowRequest {
    /// url of the document that should be processed
    document_url: String,
    /// question about the document, should be answerable with true/false
    question: String,
    /// optional tag to assign if answer is true
    true_tag: Option<String>,
    /// optional tag to assign if the answer is false
    false_tag: Option<String>,
}

#[derive(Debug, Serialize, Deserialize, ToSchema)]
struct FillAllCustomFields {
    /// url of the document that should be processed
    document_url: String,
    /// list of custom fields that will be ignored
    ///
    /// must be either an integer referencing the custom field id, or string referencing the custom field name
    ignore_custom_fields: Option<Vec<Value>>,
    /// optional tag to apply to document when finished with processing, if unspecified the configured finish tag will be set
    next_tag: Option<String>,
}

#[derive(Debug, Serialize, Deserialize, ToSchema)]
struct TargetedCustomFieldFill {
    /// url of the document that should be processed
    document_url: String,
    /// custom field id of the field to fill
    custom_field_id: i64,
    /// optional prompt to add more information to the context
    prompt: Option<String>,
    /// optional tag to apply to document when finished with processing, if unspecified the configured finish tag will be set
    next_tag: Option<String>,
}

#[derive(Debug, Serialize, Deserialize, ToSchema)]
struct TitleSuggestParams {
    /// url of the document that should be processed
    document_url: String,
    /// optional jinja-style template for title formatting, e.g. "{{correspondent}} - {{date}} - {{subject}}". If unspecified, a single free-form title is generated.
    template: Option<String>,
    /// optional tag to apply to document when finished with processing, if unspecified the configured finish tag will be set
    next_tag: Option<String>,
}

#[derive(Debug, Serialize, Deserialize, ToSchema)]
/// General webhook parameters any workflow trigger will accept this type
struct WebhookParams {
    /// url of the document that should be processed
    document_url: String,
    #[serde(default)]
    /// tag to apply to document when finished with processing, this is optional if unspecfied the configured finsh tag will be set
    next_tag: Option<String>,
}

impl ResponseError for WebhookError {
    fn status_code(&self) -> actix_web::http::StatusCode {
        match self {
            WebhookError::ReceivedRequestFromUnconfiguredServer => StatusCode::UNAUTHORIZED,
            _ => StatusCode::BAD_REQUEST,
        }
    }
}

impl WebhookParams {
    async fn handle_request(
        &self,
        status_tags: Data<PaperlessStatusTags>,
        api_client: Data<Client>,
        config: Data<Config>,
        document_pipeline: web::Data<tokio::sync::mpsc::UnboundedSender<DocumentProcessingRequest>>,
        req_type: ProcessingType,
    ) -> Result<(), WebhookError> {
        let doc_url: Uri = self.document_url.parse().unwrap();
        let configured_paperless_server: Uri = config.paperless_server.parse().unwrap();
        // verify document host and configured paperless instance are the same host to avoid handling requests from other paperless instances
        if doc_url.host().is_some_and(|rs| {
            configured_paperless_server
                .host()
                .is_some_and(|cs| cs == rs)
        }) {
            if let Some(doc_id_cap) = DOCID_REGEX.captures(doc_url.path()) {
                if let Some(doc_id) = doc_id_cap
                    .get(1)
                    .and_then(|v| v.as_str().parse::<i64>().ok())
                {
                    let mut api_client =
                        Arc::<Client>::make_mut(&mut api_client.into_inner()).clone();
                    if let Ok(mut doc) = api_client.documents().retrieve(doc_id, None, None).await {
                        // if documents has no processing tag set it
                        if !doc.tags.contains(&status_tags.processing.id) {
                            let mut updated_doc_tags = doc.tags.clone();
                            updated_doc_tags.push(status_tags.processing.id);
                            let _ = requests::update_document_tag_ids(
                                &mut api_client,
                                &mut doc,
                                &updated_doc_tags,
                            )
                            .await;
                        }
                        let mut next_tag = None;
                        if let Some(nt_to_parse) = &self.next_tag {
                            if let Ok(nt_as_id) = nt_to_parse.parse::<i64>() {
                                next_tag = requests::fetch_tag_by_id_or_name(
                                    &mut api_client,
                                    None,
                                    Some(nt_as_id),
                                )
                                .await;
                            } else {
                                next_tag = requests::fetch_tag_by_id_or_name(
                                    &mut api_client,
                                    Some(nt_to_parse.clone()),
                                    None,
                                )
                                .await;
                            }
                        }
                        if next_tag.is_none() && self.next_tag.is_some() {
                            log::warn!(
                                "Webhook received request to use specific finished tag, but the tag does not exists (next_tag=`{}`)! Ignoring tag from request!",
                                self.next_tag.as_ref().unwrap()
                            );
                        }
                        let _ = document_pipeline.send(DocumentProcessingRequest {
                            document: doc,
                            processing_type: req_type,
                            overwrite_finshed_tag: next_tag,
                        });
                        Ok(())
                    } else {
                        Err(WebhookError::DocumentDoesNotExist(doc_id))
                    }
                } else {
                    Err(WebhookError::InvalidDocumentId)
                }
            } else {
                Err(WebhookError::DocumentUrlParsingIDFailed)
            }
        } else {
            Err(WebhookError::ReceivedRequestFromUnconfiguredServer)
        }
    }
}

#[utoipa::path(tag = "llm_workflow_trigger", request_body = inline(DecisionTagFlowRequest))]
#[post("/decision")]
/// ask a question that can be answered with true or false about the document
///
/// The goal of this endpoint is to enable decision based workflows in paperless. Ask a question about the document and
/// if the answer is true the document will be assigend the `true_tag`. If not and the `false_tag` is specified it will be assigned.
/// This enables doing `if/else` style workflows by using tagging to conditionally trigger further processing steps.
///
/// If neither `false_tag` nor `true_tag` are specified the request will be discared since the result would have no effect!
async fn decision(
    params: web::Json<DecisionTagFlowRequest>,
    status_tags: Data<PaperlessStatusTags>,
    api_client: Data<Client>,
    config: Data<Config>,
    document_pipeline: web::Data<tokio::sync::mpsc::UnboundedSender<DocumentProcessingRequest>>,
) -> Result<HttpResponse, WebhookError> {
    let mut api_client_cloned =
        Arc::<Client>::make_mut(&mut api_client.clone().into_inner()).clone();
    let mut true_tag = None;
    if let Some(tt_to_parse) = &params.true_tag {
        if let Ok(tt_as_id) = tt_to_parse.parse::<i64>() {
            true_tag =
                requests::fetch_tag_by_id_or_name(&mut api_client_cloned, None, Some(tt_as_id))
                    .await;
        } else {
            true_tag = requests::fetch_tag_by_id_or_name(
                &mut api_client_cloned,
                Some(tt_to_parse.clone()),
                None,
            )
            .await;
        }
    }
    // a true tag is expected, if none could be found then the request is invalid
    if true_tag.is_none() && params.true_tag.is_some() {
        return Err(WebhookError::TagNotFound(params.true_tag.clone().unwrap()));
    }

    let mut false_tag = None;
    if let Some(ft_to_parse) = &params.false_tag {
        if let Ok(ft_as_id) = ft_to_parse.parse::<i64>() {
            false_tag =
                requests::fetch_tag_by_id_or_name(&mut api_client_cloned, None, Some(ft_as_id))
                    .await;
        } else {
            false_tag = requests::fetch_tag_by_id_or_name(
                &mut api_client_cloned,
                Some(ft_to_parse.clone()),
                None,
            )
            .await;
        }
    }

    // if the request specified a false tag but it could not be found, this is also an error
    // since the user specifically wants to have a tag on false result, if the tag does not
    // exists then this will not work, so the request is invalid
    if false_tag.is_none() && params.false_tag.is_some() {
        return Err(WebhookError::TagNotFound(params.false_tag.clone().unwrap()));
    }

    if true_tag.is_none() && false_tag.is_none() {
        return Err(WebhookError::RequestWithoutEffect);
    }

    let process_type = ProcessingType::DecsionTagFlow {
        question: params.question.clone(),
        true_tag,
        false_tag,
    };

    let generic_webhook_params = WebhookParams {
        document_url: params.document_url.clone(),
        next_tag: None,
    };

    generic_webhook_params
        .handle_request(
            status_tags,
            api_client,
            config,
            document_pipeline,
            process_type,
        )
        .await?;
    Ok(HttpResponse::Accepted().into())
}

#[utoipa::path(tag = "llm_workflow_trigger")]
#[post("/suggest/correspondent")]
/// Workflow to suggest a correspondent for a document
///
/// Given the document content and all possible correspondents select a correspondent using a
/// reasoning approach. This workflow might take longer given the llm reasoning!
///
/// Afterwards set the correspondent of the document, sadly just adding it as another suggestion is not supported
/// by the paperless api.
async fn suggest_correspondent(
    params: web::Json<WebhookParams>,
    status_tags: Data<PaperlessStatusTags>,
    api_client: Data<Client>,
    config: Data<Config>,
    document_pipeline: web::Data<tokio::sync::mpsc::UnboundedSender<DocumentProcessingRequest>>,
) -> Result<HttpResponse, WebhookError> {
    params
        .handle_request(
            status_tags,
            api_client,
            config,
            document_pipeline,
            ProcessingType::CorrespondentSuggest,
        )
        .await?;
    Ok(HttpResponse::Accepted().into())
}

#[utoipa::path(tag = "llm_workflow_trigger")]
#[post("/fill/custom_fields")]
/// Workflow to fill unfilled custom fields on a document
///
/// Scan document for unfilled custom fields and use llm to predict the values from the document content.
///
/// ## Supported Custom Field Types
///
/// Currently this projects predicting the following kinds of custom fields:
/// - [x] Boolean
/// - [x] Date
/// - [x] Integer
/// - [x] Number
/// - [x] Monetary
/// - [x] Text
/// - [x] Select
/// - [ ] Document Link
/// - [ ] URL
/// - [x] LargeText
async fn custom_field_prediction(
    params: web::Json<FillAllCustomFields>,
    status_tags: Data<PaperlessStatusTags>,
    api_client: Data<Client>,
    config: Data<Config>,
    document_pipeline: web::Data<tokio::sync::mpsc::UnboundedSender<DocumentProcessingRequest>>,
) -> Result<HttpResponse, WebhookError> {
    let exclude_fields = if let Some(ignore_custom_fields) = &params.ignore_custom_fields {
        let mut ignored_cf_ids = Vec::new();
        let mut ignored_cf_names = Vec::new();

        for ignored_field_value in ignore_custom_fields {
            match ignored_field_value {
                Value::Null | Value::Bool(_) | Value::Array(_) | Value::Object(_) => {
                    return Err(WebhookError::InvalidIgnoreCustomFieldValue);
                }
                Value::Number(number) => {
                    if number.is_i64() {
                        ignored_cf_ids.push(number.as_i64().unwrap());
                    } else if number.is_u64() {
                        ignored_cf_ids.push(number.as_u64().unwrap() as i64);
                    } else {
                        return Err(WebhookError::InvalidIgnoreCustomFieldValue);
                    }
                }
                Value::String(name) => {
                    ignored_cf_names.push(name.clone());
                }
            }
        }

        let mut api_client_cloned =
            Arc::<Client>::make_mut(&mut api_client.clone().into_inner()).clone();
        Some(
            requests::get_all_custom_fields(&mut api_client_cloned)
                .await
                .into_iter()
                .filter(|cf| ignored_cf_ids.contains(&cf.id) || ignored_cf_names.contains(&cf.name))
                .collect(),
        )
    } else {
        None
    };

    let generic_webhook_params = WebhookParams {
        document_url: params.document_url.clone(),
        next_tag: params.next_tag.clone(),
    };
    generic_webhook_params
        .handle_request(
            status_tags,
            api_client,
            config,
            document_pipeline,
            ProcessingType::CustomFieldPrediction { exclude_fields },
        )
        .await?;
    Ok(HttpResponse::Accepted().into())
}

#[utoipa::path(tag = "llm_workflow_trigger", request_body = inline(TitleSuggestParams))]
#[post("/suggest/title")]
/// Workflow to suggest a document title
///
/// Given the document content, use the LLM to generate a title. Optionally a jinja-style template
/// can be provided to control the title format, e.g. `{{correspondent}} - {{date}} - {{subject}}`.
/// When a template is used, the LLM generates each field individually and the code composes the
/// final title. Without a template, a single free-form title is generated.
async fn suggest_title(
    params: web::Json<TitleSuggestParams>,
    status_tags: Data<PaperlessStatusTags>,
    api_client: Data<Client>,
    config: Data<Config>,
    document_pipeline: web::Data<tokio::sync::mpsc::UnboundedSender<DocumentProcessingRequest>>,
) -> Result<HttpResponse, WebhookError> {
    let webhook_params = WebhookParams {
        document_url: params.document_url.clone(),
        next_tag: params.next_tag.clone(),
    };
    webhook_params
        .handle_request(
            status_tags,
            api_client,
            config,
            document_pipeline,
            ProcessingType::TitleSuggest {
                template: params.template.clone(),
            },
        )
        .await?;
    Ok(HttpResponse::Accepted().into())
}

#[derive(utoipa::OpenApi)]
#[openapi(
    paths(suggest_correspondent, custom_field_prediction, decision, suggest_title),
    components(schemas(WebhookParams, TitleSuggestParams))
)]
pub(crate) struct DocumentProcessingApiSpec;

struct DocumentProcessingApi;

impl HttpServiceFactory for DocumentProcessingApi {
    fn register(self, config: &mut actix_web::dev::AppService) {
        custom_field_prediction.register(config);
        suggest_correspondent.register(config);
        decision.register(config);
        suggest_title.register(config);
    }
}

/// given an updated document add changes from processing type to other instances of the document
///
/// the purpose of this function is the following situation, given a document may be present multiple times in the processing queue
/// and with later document versions having potentially more information in them this function should update later versions of the
/// document with data from previously run processing steps without loosing any additional data. The goal being to mininmize the amount
/// of back communication with the paperless server to avoid updating documents multiple times and triggering workflows unnecessarìly
fn merge_document_status(
    doc: &mut Document,
    updated_doc: &Document,
    processing_type: &ProcessingType,
) {
    if doc.id != updated_doc.id {
        // if document ids do not match stop here!
        return;
    }
    match processing_type {
        ProcessingType::CustomFieldPrediction { exclude_fields: _ } => {
            if let Some(updated_custom_fields) = &updated_doc.custom_fields {
                for updated_cf in updated_custom_fields {
                    if let Some(doc_custom_fields) = doc.custom_fields.as_mut() {
                        let mut cf_found = false;
                        for doc_cf_i in &mut *doc_custom_fields {
                            if doc_cf_i.field == updated_cf.field {
                                cf_found = true;
                                doc_cf_i.value = updated_cf.value.clone();
                            }
                        }
                        if !cf_found {
                            doc_custom_fields.push(updated_cf.clone());
                        }
                    }
                }
            }
        }
        ProcessingType::CorrespondentSuggest => doc.correspondent = updated_doc.correspondent,
        ProcessingType::DecsionTagFlow {
            question: _,
            true_tag: _,
            false_tag: _,
        } => {
            // this processing type adds tags to the document
            for updated_tag in &updated_doc.tags {
                if !doc.tags.contains(updated_tag) {
                    doc.tags.push(*updated_tag);
                }
            }
        }
        ProcessingType::TitleSuggest { template: _ } => {
            doc.title = updated_doc.title.clone();
        }
        ProcessingType::TargetedCustomField {
            custom_field_id,
            prompt: _,
        } => {
            if let Some(updated_custom_fields) = &updated_doc.custom_fields {
                if let Some(updated_cf) = updated_custom_fields
                    .iter()
                    .find(|cf| cf.field == *custom_field_id)
                {
                    if let Some(doc_custom_fields) = doc.custom_fields.as_mut() {
                        doc_custom_fields.retain(|cf| cf.field != *custom_field_id);
                        doc_custom_fields.push(updated_cf.clone());
                    }
                }
            }
        }
    }
}

/// this jobs function is to receive the processed and send the results back the paperless instance
///
/// the goal is to minimize traffic to the paperless instance and avoid waiting for api requests
async fn document_updater(
    status_tags: PaperlessStatusTags,
    mut api_client: Client,
    mut document_update_channel: tokio::sync::mpsc::UnboundedReceiver<
        Result<
            (DocumentProcessingRequest, bool),
            (DocumentProcessingError, DocumentProcessingRequest, bool),
        >,
    >,
) {
    let mut defered_doc_updates: BTreeMap<i64, Vec<ProcessingType>> = BTreeMap::new();
    let mut document_processing_errors: BTreeMap<i64, Vec<DocumentProcessingError>> =
        BTreeMap::new();

    while let Some(doc_update) = document_update_channel.recv().await {
        let same_doc_in_queue_again;
        let doc_req = match doc_update {
            Ok((doc_req, queued_again)) => {
                same_doc_in_queue_again = queued_again;
                doc_req
            }
            Err((err, doc_req, queued_again)) => {
                same_doc_in_queue_again = queued_again;
                if let std::collections::btree_map::Entry::Vacant(e) =
                    document_processing_errors.entry(doc_req.document.id)
                {
                    e.insert(vec![err]);
                } else if let Some(v) = document_processing_errors
                    .get_mut(&doc_req.document.id)
                    .as_mut()
                {
                    v.push(err);
                }
                doc_req
            }
        };

        // if there are now further processing steps pending for the document then it's
        // state can be synced back to the paperless server and all processing tags removed
        // finshed / next tag will be set
        if !same_doc_in_queue_again {
            let updated_doc_tags: Vec<i64> = doc_req
                .document
                .tags
                .iter()
                .copied()
                .filter(|t| *t != status_tags.processing.id)
                .chain(if doc_req.overwrite_finshed_tag.is_none() {
                    [status_tags.finished.id].into_iter()
                } else {
                    [doc_req.overwrite_finshed_tag.as_ref().unwrap().id].into_iter()
                })
                .unique()
                .collect();

            let mut updated_cf: Option<Vec<CustomFieldInstance>> = None;
            let mut updated_crrspdnt: Option<i64> = None;
            let mut updated_title: Option<String> = None;

            for doc_processing_steps in [doc_req.processing_type]
                .iter()
                .chain(
                    defered_doc_updates
                        .get(&doc_req.document.id)
                        .unwrap_or(&Vec::new()),
                )
                .unique()
            {
                match doc_processing_steps {
                    ProcessingType::CustomFieldPrediction { exclude_fields: _ } => {
                        if let Some(cfis) = doc_req.document.custom_fields.as_ref() {
                            updated_cf = Some(cfis.clone());
                        }
                    }
                    ProcessingType::CorrespondentSuggest => {
                        updated_crrspdnt = doc_req.document.correspondent;
                    }
                    ProcessingType::DecsionTagFlow {
                        question: _,
                        true_tag: _,
                        false_tag: _,
                    } => {
                        // nothing needs to happen here, the updated tags are already part of the document
                        // since they are synced with the same document in the queue
                    }
                    ProcessingType::TitleSuggest { template: _ } => {
                        if let Some(ref t) = doc_req.document.title
                            && !t.is_empty()
                        {
                            updated_title = Some(t.clone());
                        }
                    }
                    ProcessingType::TargetedCustomField {
                        custom_field_id,
                        prompt: _,
                    } => {
                        if let Some(cfis) = doc_req.document.custom_fields.as_ref()
                            && let Some(target_field_update) =
                                cfis.iter().find(|cf| cf.field == *custom_field_id)
                        {
                            let cf_update_list = updated_cf.get_or_insert_default();
                            cf_update_list.retain(|cf| cf.field != *custom_field_id);
                            cf_update_list.push(target_field_update.clone());
                        } else {
                            // TODO should something happen if the targeted field was not updated 🤔
                        }
                    }
                }
            }

            // copy tag list before update and include error tag
            // will only be used if documents has errors or update fails for some reason
            let mut updated_tag_with_error = updated_doc_tags.clone();

            let update_result = requests::processed_doc_update(
                &mut api_client,
                doc_req.document.id,
                updated_doc_tags,
                updated_crrspdnt,
                updated_title,
                updated_cf,
            )
            .await
            .map_err(|err| {
                log::error!("{err}");
                err
            });

            // error tag is only defined when features is enabled
            if let Some(ref error_tag) = status_tags.error
                // and if there where errrors during processing
                // or if the request updating the document failed
                && (!document_processing_errors
                    .get(&doc_req.document.id)
                    .is_none_or(|v| v.is_empty())
                    || update_result.is_err())
            {
                updated_tag_with_error.push(error_tag.id);
                let _ = requests::processed_doc_update(
                    &mut api_client,
                    doc_req.document.id,
                    updated_tag_with_error,
                    None,
                    None,
                    None,
                )
                .await
                .map_err(|err| {
                    // if even updating tags fails something is very wrong
                    error!(
                        "Error updating Tags for Document with id {}:\n{err:#?}\n\n This probably warrants a bug report!",
                        doc_req.document.id
                    )
                });
            }

            // if document has been synced there is no reason to hold on to the last updates
            defered_doc_updates.remove(&doc_req.document.id);
            document_processing_errors.remove(&doc_req.document.id);
        } else {
            // remember how document has been processed until now for defered update later
            if let std::collections::btree_map::Entry::Vacant(e) =
                defered_doc_updates.entry(doc_req.document.id)
            {
                e.insert(vec![doc_req.processing_type]);
            } else if defered_doc_updates
                .get(&doc_req.document.id)
                .is_some_and(|v| v.contains(&doc_req.processing_type))
            {
                continue;
            } else if let Some(v) = defered_doc_updates.get_mut(&doc_req.document.id).as_mut() {
                v.push(doc_req.processing_type);
            }
        }
    }
}

// future performance optimization needs to focus on this function, it should dispatch to batch processing of documents
// or could combine requests to the same document in the queue.
/// this jobs functions it to batch process documents using the llm and send the results on to the update handler
async fn document_processor(
    config: Config,
    mut api_client: Client,
    document_update_channel: tokio::sync::mpsc::UnboundedSender<
        Result<
            (DocumentProcessingRequest, bool),
            (DocumentProcessingError, DocumentProcessingRequest, bool),
        >,
    >,
) {
    while !*STOP_FLAG.read().await {
        while !PROCESSING_QUEUE.read().await.is_empty() {
            let model_path = config.model.clone();
            {
                let mut model_singleton = MODEL_SINGLETON.lock().await;
                if model_singleton.is_none() {
                    let max_ctx = if config.max_ctx == 0 {
                        None
                    } else {
                        Some(config.max_ctx as u32)
                    };
                    *model_singleton = spawn_blocking(move || {
                        LLModelExtractor::new(
                            Path::new(&model_path),
                            config.num_gpu_layers,
                            max_ctx,
                        )
                    })
                    .await
                    .map_err(|err| {
                        log::error!("Error loading Model! {err}");
                        ModelError::ModelNotLoaded
                    })
                    .and_then(|r| r)
                    .ok();
                }
            }
            let mut doc_process_req = {
                // nesting here to ensure write lock is dropped while processing the document in the next step
                PROCESSING_QUEUE
                    .write()
                    .await
                    .pop_front()
                    .expect("Size is greater 0 so there must be a document in the queue")
            };

            let processing_result = match doc_process_req.processing_type {
                ProcessingType::CustomFieldPrediction { ref exclude_fields } => {
                    handle_custom_field_prediction(
                        &mut doc_process_req.document,
                        exclude_fields,
                        &mut api_client,
                    )
                    .await
                }
                ProcessingType::CorrespondentSuggest => {
                    handle_correspondend_suggest(&mut doc_process_req.document, &mut api_client)
                        .await
                }
                ProcessingType::DecsionTagFlow {
                    ref question,
                    ref true_tag,
                    ref false_tag,
                } => {
                    handle_decision(
                        &mut doc_process_req.document,
                        question,
                        true_tag.as_ref(),
                        false_tag.as_ref(),
                    )
                    .await
                }
                ProcessingType::TitleSuggest { ref template } => {
                    handle_title_suggestion(
                        &mut doc_process_req.document,
                        &mut api_client,
                        template,
                    )
                    .await
                }
                ProcessingType::TargetedCustomField {
                    custom_field_id,
                    prompt,
                } => unimplemented!(),
            };

            let mut doc_in_queue_again = false;
            match processing_result {
                Ok(_) => {
                    // if the same document has more processing requests pending update the doc state to
                    // also contain the newly added data.
                    for next_process_req in PROCESSING_QUEUE.write().await.iter_mut() {
                        if next_process_req.document.id == doc_process_req.document.id {
                            doc_in_queue_again = true;
                            merge_document_status(
                                &mut next_process_req.document,
                                &doc_process_req.document,
                                &doc_process_req.processing_type,
                            );
                        }
                    }
                    let _ = document_update_channel.send(Ok((doc_process_req, doc_in_queue_again)));
                }
                Err(err) => {
                    doc_in_queue_again = PROCESSING_QUEUE
                        .read()
                        .await
                        .iter()
                        .map(|doc_req| doc_req.document.id)
                        .contains(&doc_process_req.document.id);
                    let _ = document_update_channel.send(Err((
                        err,
                        doc_process_req,
                        doc_in_queue_again,
                    )));
                }
            }
        }
        tokio::time::sleep(Duration::from_secs(2)).await;
        if PROCESSING_QUEUE.read().await.is_empty() && MODEL_SINGLETON.lock().await.is_some() {
            // No Documents need processing drop model
            log::info!("Unloading Model due to processing queue being empty!");
            let mut model_singleton = MODEL_SINGLETON.lock().await;
            let _ = model_singleton.take();
        }
    }
}

/// this function is just here to receive documents for processing from the different api endpoints
/// all documents received via the channel are put into a linked list of documents that need processing
/// the reason for this is that this way the document processor can inspect the state of the document queue and
/// make smart decisions on how to process the documents for maximum efficiency
async fn document_request_funnel(
    mut processing_request_channel: tokio::sync::mpsc::UnboundedReceiver<DocumentProcessingRequest>,
) {
    while let Some(prc_req) = processing_request_channel.recv().await {
        log::debug!("Received Request for Document {:#?}", prc_req.document.id);
        let mut doc_queue = PROCESSING_QUEUE.write().await;
        doc_queue.push_back(prc_req);
    }

    *STOP_FLAG.write().await = true;
}

#[derive(Debug, Clone)]
struct PaperlessStatusTags {
    processing: Tag,
    finished: Tag,
    error: Option<Tag>,
}

pub async fn run_server(
    config: Config,
    processing_tag: Tag,
    finished_tag: Tag,
    error_tag: Option<Tag>,
    paperless_api_client: Client,
) -> Result<(), std::io::Error> {
    let (tx, rx) = tokio::sync::mpsc::unbounded_channel::<DocumentProcessingRequest>();
    let (tx_update, rx_update) = tokio::sync::mpsc::unbounded_channel();

    let status_tags = PaperlessStatusTags {
        processing: processing_tag,
        finished: finished_tag,
        error: error_tag,
    };

    let doc_to_process_queue = spawn(document_request_funnel(rx));
    let doc_processor = spawn(document_processor(
        config.clone(),
        paperless_api_client.clone(),
        tx_update,
    ));
    let doc_update_task = spawn(document_updater(
        status_tags.clone(),
        paperless_api_client.clone(),
        rx_update,
    ));
    let server_config = config.clone();
    let webhook_server = HttpServer::new(move || {
        App::new()
            .app_data(Data::new(tx.clone()))
            .app_data(Data::new(server_config.clone()))
            .app_data(Data::new(paperless_api_client.clone()))
            .app_data(Data::new(status_tags.clone()))
            .service(
                SwaggerUi::new("/api/{_:.*}")
                    .config(utoipa_swagger_ui::Config::default().use_base_layout())
                    .url("/docs/openapi.json", DocumentProcessingApiSpec::openapi()),
            )
            .service(DocumentProcessingApi)
    })
    .bind((config.host, config.port))?
    .run();

    let _ = join!(
        webhook_server,
        doc_to_process_queue,
        doc_processor,
        doc_update_task
    );

    Ok(())
}
