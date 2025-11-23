use chrono::NaiveDate;
use paperless_api_client::types::{Correspondent, CustomField, CustomFieldInstance, DataTypeEnum};
use schemars::{JsonSchema, json_schema, schema_for};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use thiserror::Error;

#[derive(Serialize, Deserialize, JsonSchema)]
/// Structure to extract currency data from a document
pub(crate) struct CurrencyValue {
    value: f64,
    currency_code: String,
}

#[derive(Debug, Serialize, Deserialize, JsonSchema)]
pub(crate) struct FieldExtract {
    /// this field is used to guide the model to extract the desired data
    /// during grammar generation the string will be set to a constant value
    /// with the content being the name of the custom field that is to be extracted
    description: String,
    /// this field may hold extra information to guide the model towards the desired output
    /// it will be filled with extra information as a constant. For example it will hold
    /// all allowed variants when expecting a enum as schema for field_value or describe the
    /// format of a date output. When no extra constraints apply it will be ommitted from
    /// the grammar.
    #[serde(default)]
    format: Option<Value>,
    /// since the custom field can hold any kind of data a generic json value is required to
    /// to hold it. During grammar generation the type of this value will be replaced with
    /// the type of the custom field
    pub value: Value,
    // as with `field_value` the element type will be replaced during grammar generation
    // to correspond to the type of the custom field. This field currently does not really
    // do much, the idea would be to use it in training for reward models if output the correct field
    // here although it might have put the wrong value in the actual value field. Not sure if this makes
    // sense though. Another idea would be to add these as suggestions to paperless, but for that to work
    // custom field suggestions would need to be implemented for paperless first. I don't think they are
    // at the moment
    //alternative_values: Vec<Value>,
}

#[derive(Debug, Error)]
pub(crate) enum FieldError {
    #[error("custom fields with type {0} are not supported!")]
    UnsupportedCustomFieldType(DataTypeEnum),
    #[error("could not parse custom field value to the expected custom field type")]
    ParsingError(#[from] serde_json::error::Error),
    #[error("custom field `{0}` is of type enum but has no variants defined!")]
    EmptyEnumValuesForCustomField(String),
    #[error("matching {0} variant does not uniquely match any variant of custom field {1}")]
    NoUniqueEnumValueFound(String, String),
    #[error("could not match {0} to any known correspondent!")]
    CorrespondentNotFound(String),
}

impl FieldExtract {
    pub fn to_correspondent(
        &self,
        all_correspondents: &[Correspondent],
    ) -> Result<Correspondent, FieldError> {
        let parsed_value: String = serde_json::from_value(self.value.clone())?;

        all_correspondents
            .iter()
            .find(|c| c.name == parsed_value)
            .ok_or(FieldError::CorrespondentNotFound(parsed_value))
            .cloned()
    }
    pub fn to_custom_field_instance(
        &self,
        custom_field_spec: &CustomField,
    ) -> Result<CustomFieldInstance, FieldError> {
        // try to parse the value into the specified type and transform
        // it to the representation required for paperless to set
        // custom field based via the API
        match custom_field_spec.data_type {
            paperless_api_client::types::DataTypeEnum::Boolean => {
                let _parsed_value: bool = serde_json::from_value(self.value.clone())?;
                Ok(CustomFieldInstance {
                    value: Some(self.value.clone()),
                    field: custom_field_spec.id,
                })
            }
            paperless_api_client::types::DataTypeEnum::String => {
                let _parsed_value: String = serde_json::from_value(self.value.clone())?;
                Ok(CustomFieldInstance {
                    value: Some(self.value.clone()),
                    field: custom_field_spec.id,
                })
            }
            paperless_api_client::types::DataTypeEnum::Date => {
                let _parsed_value: NaiveDate = serde_json::from_value(self.value.clone())?;
                Ok(CustomFieldInstance {
                    value: Some(self.value.clone()),
                    field: custom_field_spec.id,
                })
            }
            paperless_api_client::types::DataTypeEnum::Integer => {
                let _parsed_value: i64 = serde_json::from_value(self.value.clone())?;
                Ok(CustomFieldInstance {
                    value: Some(self.value.clone()),
                    field: custom_field_spec.id,
                })
            }
            paperless_api_client::types::DataTypeEnum::Float => {
                let _parsed_value: f64 = serde_json::from_value(self.value.clone())?;
                Ok(CustomFieldInstance {
                    value: Some(self.value.clone()),
                    field: custom_field_spec.id,
                })
            }
            paperless_api_client::types::DataTypeEnum::Monetary => {
                let parsed_currency_value: CurrencyValue =
                    serde_json::from_value(self.value.clone())?;
                Ok(CustomFieldInstance {
                    value: Some(Value::String(format!(
                        "{}{:.2}",
                        parsed_currency_value.currency_code, parsed_currency_value.value
                    ))),
                    field: custom_field_spec.id,
                })
            }
            paperless_api_client::types::DataTypeEnum::Select => {
                let enum_variant: String = serde_json::from_value(self.value.clone())?;
                let select_options: FieldSelect = if let Some(v) = &custom_field_spec.extra_data {
                    serde_json::from_value(v.clone()).unwrap()
                } else {
                    return Err(FieldError::EmptyEnumValuesForCustomField(
                        custom_field_spec.name.clone(),
                    ));
                };
                let matching_enum_values = &select_options
                    .select_options
                    .into_iter()
                    .filter(|cfi| *cfi.label == enum_variant)
                    .collect::<Vec<_>>();
                if matching_enum_values.len() != 1 {
                    Err(FieldError::NoUniqueEnumValueFound(
                        enum_variant,
                        custom_field_spec.name.clone(),
                    ))
                } else {
                    let matching_enum_variant = matching_enum_values
                        .first()
                        .expect("if case checks that exactly one element exists");
                    Ok(CustomFieldInstance {
                        // the value of a custom field corresponds to the string id of the the enum variant field
                        value: Some(serde_json::Value::String(matching_enum_variant.id.clone())),
                        field: custom_field_spec.id,
                    })
                }
            }
            paperless_api_client::types::DataTypeEnum::Url
            | paperless_api_client::types::DataTypeEnum::Documentlink => Err(
                FieldError::UnsupportedCustomFieldType(custom_field_spec.data_type.clone()),
            ),
        }
        //TODO figure out what to do with alternative value suggestions
    }
}

#[derive(Serialize, Deserialize)]
pub(crate) struct SelectOption {
    id: String,
    label: String,
}

#[derive(Serialize, Deserialize)]
pub(crate) struct FieldSelect {
    select_options: Vec<SelectOption>,
}

pub(crate) fn custom_field_learning_supported(cf: &CustomField) -> bool {
    !matches!(cf.data_type, DataTypeEnum::Documentlink | DataTypeEnum::Url)
}

#[derive(Debug)]
struct GuideDef {
    name: String,
    value: Value,
}

/// Some custom fields require some extra guidance for the model to be able to
/// produce sensible output. The guide fields purpose is to contrain the probability
/// space further. This function handles creation of this guide based on the custom
/// fields definition.
fn guide_value_from_custom_field(cf: &CustomField) -> Option<GuideDef> {
    match cf.data_type {
        DataTypeEnum::String
        | DataTypeEnum::Boolean
        | DataTypeEnum::Integer
        | DataTypeEnum::Float
        | DataTypeEnum::Monetary => {
            // base types where constraining apart
            // from using the custom field name
            // is not possible
            None
        }
        DataTypeEnum::Date => {
            // expect the date as rfc3339 format
            Some(GuideDef {
                name: "format".to_string(),
                value: json!("rfc3339"),
            })
        }
        DataTypeEnum::Select => {
            let select_options: FieldSelect = if let Some(v) = &cf.extra_data {
                serde_json::from_value(v.clone()).unwrap()
            } else {
                // this case should not occur because it means the custom field has
                // no allowed variants …
                return None;
            };
            let enum_values = serde_json::to_value(
                select_options
                    .select_options
                    .into_iter()
                    .map(|o| o.label)
                    .collect::<Vec<_>>(),
            )
            .unwrap();
            // expect value to be one of the allowed variants
            Some(GuideDef {
                name: "one_of".to_string(),
                value: enum_values,
            })
        }
        DataTypeEnum::Url | DataTypeEnum::Documentlink => {
            // currently unsupported custom fields
            // will probably require some guidance when implemented
            None
        }
    }
}

pub(crate) fn schema_from_correspondents(crrspd_list: &[Correspondent]) -> schemars::Schema {
    let correspondend_name_list: Vec<Value> = crrspd_list
        .iter()
        .map(|crrspd| json!(crrspd.name))
        .collect();

    let type_allowed_correspondens = json_schema!({
        "type": "string",
        "enum": correspondend_name_list
    });

    let mut base_schema = schema_for!(FieldExtract);
    if let Some(properties) = base_schema.get_mut("properties") {
        if let Some(description_schema) = properties.get_mut("description") {
            *description_schema = json_schema!({ "const": "Correspondent" })
                .as_value()
                .clone();
        }
        if let Some(legend_schema) = properties.get_mut("format") {
            *legend_schema = json_schema!({
                "type": "object",
                "properties": {
                    "one_of": { "const": correspondend_name_list }
                },
                "required": [
                    "one_of"
                ]
            })
            .as_value()
            .clone();
        }
        if let Some(prop) = properties.as_object_mut() {
            let key_name = "most_likely_value_reasoning_summarized";
            prop.shift_insert(
                2,
                key_name.to_string(),
                json_schema!({ "type": "string" }).to_value(),
            );
            prop.get_mut("required")
                .map(|required| required.as_array_mut().map(|rv| rv.push(json!(key_name))));
        }
        if let Some(value_schema) = properties.get_mut("value") {
            *value_schema = type_allowed_correspondens.as_value().clone();
        }
        if let Some(array) = properties.get_mut("alternative_values")
            && let Some(value_schema) = array.get_mut("items")
        {
            *value_schema = type_allowed_correspondens.as_value().clone();
        }
    }
    base_schema
}

pub(crate) fn schema_from_custom_field(cf: &CustomField) -> Option<schemars::Schema> {
    let mut base_schema = schema_for!(FieldExtract);
    // set field of description schema as a constant string value matching the name
    // of the custom field. This should guide the llm token generation to extract the
    // desired information from the document
    if let Some(properties) = base_schema.get_mut("properties")
        && let Some(description_schema) = properties.get_mut("description")
    {
        *description_schema = json_schema!({ "const": cf.name }).as_value().clone();
    }
    let field_schema = match cf.data_type {
        paperless_api_client::types::DataTypeEnum::String => schema_for!(String),
        paperless_api_client::types::DataTypeEnum::Date => schema_for!(chrono::NaiveDate),
        paperless_api_client::types::DataTypeEnum::Boolean => schema_for!(bool),
        paperless_api_client::types::DataTypeEnum::Integer => schema_for!(i64),
        paperless_api_client::types::DataTypeEnum::Float => schema_for!(f64),
        paperless_api_client::types::DataTypeEnum::Monetary => schema_for!(CurrencyValue),
        paperless_api_client::types::DataTypeEnum::Select => {
            let select_options: FieldSelect = if let Some(v) = &cf.extra_data {
                serde_json::from_value(v.clone()).unwrap()
            } else {
                return None;
            };
            let enum_values = serde_json::to_value(
                select_options
                    .select_options
                    .into_iter()
                    .map(|o| o.label)
                    .collect::<Vec<_>>(),
            )
            .unwrap();
            json_schema!({
                "type": "string",
                "enum": enum_values
            })
        }
        paperless_api_client::types::DataTypeEnum::Url
        | paperless_api_client::types::DataTypeEnum::Documentlink => {
            return None;
        }
    };
    if let Some(guide_value) = guide_value_from_custom_field(cf) {
        base_schema.get_mut("properties").map(|properties| {
            properties.get_mut("format").map(|legend_schema| {
                *legend_schema = json_schema!({
                    "type": "object",
                    "properties": {
                        guide_value.name.clone(): { "const": guide_value.value }
                    },
                    "required": [
                        guide_value.name
                    ]
                })
                .as_value()
                .clone();
            })
        });
    } else {
        // remove field_legend from schema
        if let Some(properties) = base_schema.get_mut("properties")
            && let Some(prop) = properties.as_object_mut()
        {
            prop.remove("format");
        }
    }
    // set the schema of the field value according to the type of custom field
    if let Some(properties) = base_schema.get_mut("properties") {
        if let Some(value_schema) = properties.get_mut("value") {
            *value_schema = field_schema.as_value().clone();
        }
        if let Some(array) = properties.get_mut("alternative_values")
            && let Some(value_schema) = array.get_mut("items")
        {
            *value_schema = field_schema.as_value().clone();
        }
    }
    Some(base_schema)
}

/// the purpose of this type is to frame the language models output when handling a decision request
#[derive(Debug, Serialize, Deserialize, JsonSchema)]
pub(crate) struct Decision {
    /// the schema for this field will be replaced by a constant string that is the user supplied question about
    /// the document
    question: String,
    /// give the model some room to argue in a short string about the correct result
    answer_reasoning_short_summary: String,
    /// field that will determine the actual result
    pub answer_bool: bool,
}

pub(crate) fn schema_from_decision_question(question: &String) -> schemars::Schema {
    let mut base_schema = schema_for!(Decision);
    base_schema.get_mut("properties").map(|properties| {
        properties.get_mut("question").map(|question_schema| {
            *question_schema = json_schema!({"const": question}).as_value().clone()
        })
    });
    base_schema
}
