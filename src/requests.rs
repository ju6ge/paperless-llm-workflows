use futures::StreamExt;
use itertools::any;
use log::{error, info};
use paperless_api_client::{
    Client,
    types::{
        Correspondent, CustomField, CustomFieldInstance, CustomFieldInstanceRequest, Document,
        PatchedDocumentRequest, Suggestions, Tag, TagRequest, User, Workflow,
    },
};

#[allow(deprecated)]
pub async fn processed_doc_update(
    client: &mut Client,
    doc_id: i64,
    tags: Vec<i64>,
    correspondent: Option<i64>,
    title: Option<String>,
    cf: Option<Vec<CustomFieldInstance>>,
) -> Result<(), paperless_api_client::types::error::Error> {
    client
        .documents()
        .partial_update(
            doc_id,
            &PatchedDocumentRequest {
                correspondent,
                document_type: None,
                storage_path: None,
                title,
                content: None,
                tags: Some(tags),
                created: None,
                created_date: None,
                deleted_at: None,
                archive_serial_number: None,
                owner: None,
                set_permissions: None,
                custom_fields: cf.map(|cfis| {
                    cfis.into_iter()
                        .map(|cfi| CustomFieldInstanceRequest {
                            value: cfi.value,
                            field: cfi.field,
                        })
                        .collect()
                }),
                remove_inbox_tags: None,
            },
        )
        .await?;
    Ok(())
}

#[allow(dead_code)]
pub async fn get_all_custom_fields(client: &mut Client) -> Vec<CustomField> {
    info!("Requesting Custom Fields from Server");
    client
        .custom_fields()
        .list_stream(
            Default::default(),
            Default::default(),
            Default::default(),
            Default::default(),
            Default::default(),
            Default::default(),
            Default::default(),
            Default::default(),
        )
        .filter_map(async |cf_result| {
            cf_result
                .map_err(|err| {
                    error!("{err}");
                    err
                })
                .ok()
        })
        .collect()
        .await
}

pub async fn get_generated_workflow_for_custom_fields<'a>(
    client: &mut Client,
    custom_fields: &'a [CustomField],
) -> Vec<(Workflow, &'a CustomField)> {
    info!("Fetching workflows from server");
    client
        .workflows()
        .list_stream(None)
        .filter_map(async |workflow_result| {
            let workflow = workflow_result.ok()?;
            if workflow.name.starts_with("🧠") {
                for cf in custom_fields {
                    if workflow.name.contains(&cf.name) {
                        return Some((workflow, cf));
                    }
                }
                None
            } else {
                None
            }
        })
        .collect()
        .await
}

pub async fn fetch_tag_by_id_or_name(
    client: &mut Client,
    name: Option<String>,
    id: Option<i64>,
) -> Option<Tag> {
    let found_tags: Vec<Tag> = client
        .tags()
        .list_stream(None, id, None, None, None, None, name, None, None, None)
        .filter_map(async |tag_result| {
            tag_result
                .map_err(|err| {
                    log::error!("{err}");
                    err
                })
                .ok()
        })
        .collect()
        .await;
    found_tags.into_iter().next()
}

pub async fn get_custom_fields_by_id(
    client: &mut Client,
    custom_field_ids: Vec<i64>,
) -> Vec<CustomField> {
    let maybe_all_cfs = client
        .custom_fields()
        .list_stream(
            None,
            Some(custom_field_ids.clone()),
            None,
            None,
            None,
            None,
            None,
            None,
        )
        .collect::<Vec<_>>()
        .await;
    if maybe_all_cfs.iter().any(|c| c.is_err()) {
        // a single deserialization failure result in the entire page of custom fields failing to parse
        // fall back to fetch custom_fields one by one
        futures::stream::iter(custom_field_ids)
            .map(async |cf_id| (client.custom_fields().retrieve(cf_id).await, cf_id))
            .filter_map(async |v| {
                let (result, cf_id) = v.await;
                if let Err(err) = &result {
                    error!("Error fetching custom field with {cf_id}:\n{err}")
                }
                result.ok()
            })
            .collect()
            .await
    } else {
        // there won't be an error to log otherwise we are in the other branch
        maybe_all_cfs.into_iter().filter_map(|cf| cf.ok()).collect()
    }
}

pub async fn get_all_tags(client: &mut Client) -> Vec<Tag> {
    info!("Requesting All Tags from Server");
    client
        .tags()
        .list_stream(
            Default::default(),
            Default::default(),
            Default::default(),
            Default::default(),
            Default::default(),
            Default::default(),
            Default::default(),
            Default::default(),
            Default::default(),
            Default::default(),
        )
        .filter_map(async |tag_result| {
            tag_result
                .map_err(|err| {
                    error!("{err}");
                    err
                })
                .ok()
        })
        .collect()
        .await
}

#[allow(dead_code)]
pub async fn get_all_docs(client: &mut Client) -> Vec<Document> {
    client
        .documents()
        .list_stream(
            Default::default(),
            Default::default(),
            Default::default(),
            Default::default(),
            Default::default(),
            Default::default(),
            Default::default(),
            Default::default(),
            Default::default(),
            Default::default(),
            Default::default(),
            Default::default(),
            Default::default(),
            Default::default(),
            Default::default(),
            Default::default(),
            Default::default(),
            Default::default(),
            Default::default(),
            Default::default(),
            Default::default(),
            Default::default(),
            Default::default(),
            Default::default(),
            Default::default(),
            Default::default(),
            Default::default(),
            Default::default(),
            Default::default(),
            Default::default(),
            Default::default(),
            Default::default(),
            Default::default(),
            Default::default(),
            Default::default(),
            Default::default(),
            Default::default(),
            Default::default(),
            Default::default(),
            Default::default(),
            Default::default(),
            Default::default(),
            Default::default(),
            Default::default(),
            Default::default(),
            Default::default(),
            Default::default(),
            Default::default(),
            Default::default(),
            Default::default(),
            Default::default(),
            Default::default(),
            Default::default(),
            Default::default(),
            Default::default(),
            Default::default(),
            Default::default(),
            Default::default(),
            Default::default(),
            Default::default(),
            Default::default(),
            Default::default(),
            Default::default(),
            Default::default(),
            Default::default(),
            Default::default(),
            Default::default(),
            Default::default(),
            Default::default(),
            Default::default(),
            Default::default(),
            Default::default(),
            Default::default(),
            Default::default(),
            Default::default(),
            Default::default(),
            Default::default(),
            Default::default(),
            Default::default(),
            Default::default(),
            Default::default(),
            Default::default(),
            Default::default(),
            Default::default(),
            Default::default(),
            Default::default(),
            Default::default(),
            Default::default(),
            Default::default(),
            Default::default(),
            Default::default(),
            Default::default(),
            Default::default(),
            Default::default(),
            Default::default(),
            Default::default(),
            Default::default(),
            Default::default(),
            Default::default(),
            Default::default(),
            Default::default(),
            Default::default(),
            Default::default(),
            Default::default(),
            Default::default(),
            Default::default(),
            Default::default(),
            Default::default(),
            Default::default(),
            Default::default(),
        )
        .filter_map(async |doc_request| {
            doc_request
                .map_err(|err| {
                    error!("{err}");
                    err
                })
                .ok()
        })
        .collect()
        .await
}

pub(crate) async fn get_all_users(api_client: &mut Client) -> Vec<User> {
    api_client
        .users()
        .list_stream(
            Default::default(),
            Default::default(),
            Default::default(),
            Default::default(),
            Default::default(),
            Default::default(),
        )
        .filter_map(async |user_request| {
            user_request
                .map_err(|err| {
                    error!("{err}");
                    err
                })
                .ok()
        })
        .collect()
        .await
}

pub(crate) async fn create_tag(
    api_client: &mut Client,
    tag_user: Option<&User>,
    tag_name: &str,
    tag_color: &str,
) -> Result<Tag, paperless_api_client::types::error::Error> {
    api_client
        .tags()
        .create(&TagRequest {
            name: tag_name.to_owned(),
            color: Some(tag_color.to_owned()),
            match_: Some("".to_string()),
            matching_algorithm: Some(paperless_api_client::types::MatchingAlgorithm::None),
            is_insensitive: Some(true),
            is_inbox_tag: Some(false),
            owner: tag_user.map(|u| u.id),
            set_permissions: None,
            parent: None,
        })
        .await
}

#[allow(deprecated)]
#[allow(dead_code)]
pub(crate) async fn update_document_tags(
    api_client: &mut Client,
    doc: &mut Document,
    tags: &[&Tag],
) -> Result<(), paperless_api_client::types::error::Error> {
    *doc = api_client
        .documents()
        .partial_update(
            doc.id,
            &PatchedDocumentRequest {
                tags: Some(tags.iter().map(|t| t.id).collect()),
                correspondent: Default::default(),
                document_type: Default::default(),
                storage_path: Default::default(),
                title: Default::default(),
                content: Default::default(),
                created: Default::default(),
                created_date: Default::default(),
                deleted_at: Default::default(),
                archive_serial_number: Default::default(),
                owner: Default::default(),
                set_permissions: Default::default(),
                custom_fields: Default::default(),
                remove_inbox_tags: Default::default(),
            },
        )
        .await?;
    Ok(())
}

#[allow(deprecated)]
pub(crate) async fn update_document_tag_ids(
    api_client: &mut Client,
    doc: &mut Document,
    tags: &[i64],
) -> Result<(), paperless_api_client::types::error::Error> {
    *doc = api_client
        .documents()
        .partial_update(
            doc.id,
            &PatchedDocumentRequest {
                tags: Some(tags.to_vec()),
                correspondent: Default::default(),
                document_type: Default::default(),
                storage_path: Default::default(),
                title: Default::default(),
                content: Default::default(),
                created: Default::default(),
                created_date: Default::default(),
                deleted_at: Default::default(),
                archive_serial_number: Default::default(),
                owner: Default::default(),
                set_permissions: Default::default(),
                custom_fields: Default::default(),
                remove_inbox_tags: Default::default(),
            },
        )
        .await?;
    Ok(())
}

#[allow(dead_code)]
#[allow(deprecated)]
pub(crate) async fn update_document_custom_fields(
    api_client: &mut Client,
    doc: &mut Document,
    custom_fields: &[CustomFieldInstance],
) -> Result<(), paperless_api_client::types::error::Error> {
    *doc = api_client
        .documents()
        .partial_update(
            doc.id,
            &PatchedDocumentRequest {
                custom_fields: Some(
                    custom_fields
                        .iter()
                        .map(|cf| CustomFieldInstanceRequest {
                            value: cf.value.clone(),
                            field: cf.field,
                        })
                        .collect(),
                ),
                tags: Default::default(),
                correspondent: Default::default(),
                document_type: Default::default(),
                storage_path: Default::default(),
                title: Default::default(),
                content: Default::default(),
                created: Default::default(),
                created_date: Default::default(),
                deleted_at: Default::default(),
                archive_serial_number: Default::default(),
                owner: Default::default(),
                set_permissions: Default::default(),
                remove_inbox_tags: Default::default(),
            },
        )
        .await?;
    Ok(())
}

pub(crate) async fn fetch_all_correspondents(api_client: &mut Client) -> Vec<Correspondent> {
    api_client
        .correspondents()
        .list_stream(
            Default::default(),
            Default::default(),
            Default::default(),
            Default::default(),
            Default::default(),
            Default::default(),
            Default::default(),
            Default::default(),
            Default::default(),
        )
        .filter_map(async |crrspd_req| {
            crrspd_req
                .map_err(|err| {
                    error!("{err}");
                    err
                })
                .ok()
        })
        .collect()
        .await
}

#[allow(dead_code)]
pub(crate) async fn fetch_doc_suggestions(
    api_client: &mut Client,
    doc: &Document,
) -> Option<Suggestions> {
    api_client
        .documents()
        .suggestions_retrieve(doc.id)
        .await
        .map_err(|err| {
            log::error!("{err}");
            err
        })
        .ok()
}

#[allow(dead_code)]
#[allow(deprecated)]
pub(crate) async fn update_doc_correspondent(
    api_client: &mut Client,
    doc: &Document,
    correspondent: &Correspondent,
) -> Result<(), paperless_api_client::types::error::Error> {
    api_client
        .documents()
        .partial_update(
            doc.id,
            &PatchedDocumentRequest {
                correspondent: Some(correspondent.id),
                document_type: Default::default(),
                storage_path: Default::default(),
                title: Default::default(),
                content: Default::default(),
                tags: Default::default(),
                created: Default::default(),
                created_date: Default::default(),
                deleted_at: Default::default(),
                archive_serial_number: Default::default(),
                owner: Default::default(),
                set_permissions: Default::default(),
                custom_fields: Default::default(),
                remove_inbox_tags: Default::default(),
            },
        )
        .await?;
    Ok(())
}
