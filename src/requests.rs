use futures::StreamExt;
use log::{error, info};
use paperless_api_client::{
    Client,
    types::{
        Correspondent, CustomField, CustomFieldInstance, CustomFieldInstanceRequest, Document,
        PatchedDocumentRequest, Suggestions, Tag, TagRequest, User,
    },
};

#[allow(deprecated)]
pub async fn processed_doc_update(
    client: &mut Client,
    doc_id: i64,
    tags: Vec<i64>,
    correspondent: Option<i64>,
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
                title: None,
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

pub async fn fetch_tag_by_id_or_name(
    client: &mut Client,
    name: Option<String>,
    id: Option<i64>,
) -> Option<Tag> {
    let found_tags: Vec<Tag> = client
        .tags()
        .list_stream(None, id, None, None, None, name, None, None, None)
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
    custom_field_ids: Option<Vec<i64>>,
) -> Vec<CustomField> {
    client
        .custom_fields()
        .list_stream(None, custom_field_ids, None, None, None, None, None, None)
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
            matching_algorithm: Some(0),
            is_insensitive: Some(true),
            is_inbox_tag: Some(false),
            owner: tag_user.map(|u| u.id),
            set_permissions: None,
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
