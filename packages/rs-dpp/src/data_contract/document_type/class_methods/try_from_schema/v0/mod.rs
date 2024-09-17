#[cfg(feature = "validation")]
use crate::consensus::basic::data_contract::{
    DuplicateIndexNameError, InvalidIndexPropertyTypeError, InvalidIndexedPropertyConstraintError,
    SystemPropertyIndexAlreadyPresentError, UndefinedIndexPropertyError,
    UniqueIndicesLimitReachedError,
};
#[cfg(feature = "validation")]
use crate::consensus::ConsensusError;
use crate::data_contract::document_type::array::ArrayItemType;
use crate::data_contract::document_type::index::Index;
use crate::data_contract::document_type::index_level::IndexLevel;
use crate::data_contract::document_type::property::{DocumentProperty, DocumentPropertyType};
#[cfg(feature = "validation")]
use crate::data_contract::document_type::schema::validate_max_depth;
use crate::data_contract::document_type::v0::DocumentTypeV0;
#[cfg(feature = "validation")]
use crate::data_contract::document_type::v0::StatelessJsonSchemaLazyValidator;
use indexmap::IndexMap;
#[cfg(feature = "validation")]
use std::collections::HashSet;
use std::collections::{BTreeMap, BTreeSet};
use std::convert::TryInto;

#[cfg(feature = "validation")]
use crate::consensus::basic::data_contract::ContestedUniqueIndexOnMutableDocumentTypeError;
#[cfg(feature = "validation")]
use crate::consensus::basic::data_contract::ContestedUniqueIndexWithUniqueIndexError;
#[cfg(any(test, feature = "validation"))]
use crate::consensus::basic::data_contract::InvalidDocumentTypeNameError;
#[cfg(feature = "validation")]
use crate::consensus::basic::document::MissingPositionsInDocumentTypePropertiesError;
#[cfg(feature = "validation")]
use crate::consensus::basic::BasicError;
use crate::data_contract::document_type::class_methods::{
    consensus_or_protocol_data_contract_error, consensus_or_protocol_value_error,
};
use crate::data_contract::document_type::property_names::{
    CAN_BE_DELETED, CREATION_RESTRICTION_MODE, DOCUMENTS_KEEP_HISTORY, DOCUMENTS_MUTABLE,
    TRADE_MODE, TRANSFERABLE,
};
use crate::data_contract::document_type::{
    property_names, ByteArrayPropertySizes, DocumentType, StringPropertySizes,
};
use crate::data_contract::errors::DataContractError;
use crate::data_contract::storage_requirements::keys_for_document_type::StorageKeyRequirements;
use crate::identity::SecurityLevel;
use crate::util::json_schema::resolve_uri;
#[cfg(feature = "validation")]
use crate::validation::meta_validators::DOCUMENT_META_SCHEMA_V0;
use crate::validation::operations::ProtocolValidationOperation;
use crate::version::PlatformVersion;
use crate::ProtocolError;
use platform_value::btreemap_extensions::BTreeValueMapHelper;
use platform_value::{Identifier, Value};

const NOT_ALLOWED_SYSTEM_PROPERTIES: [&str; 1] = ["$id"];

const SYSTEM_PROPERTIES: [&str; 11] = [
    "$id",
    "$ownerId",
    "$createdAt",
    "$updatedAt",
    "$transferredAt",
    "$createdAtBlockHeight",
    "$updatedAtBlockHeight",
    "$transferredAtBlockHeight",
    "$createdAtCoreBlockHeight",
    "$updatedAtCoreBlockHeight",
    "$transferredAtCoreBlockHeight",
];

const MAX_INDEXED_STRING_PROPERTY_LENGTH: u16 = 63;
const MAX_INDEXED_BYTE_ARRAY_PROPERTY_LENGTH: u16 = 255;
const MAX_INDEXED_ARRAY_ITEMS: usize = 1024;

impl DocumentTypeV0 {
    // TODO: Split into multiple functions
    #[allow(unused_variables)]
    pub(super) fn try_from_schema_v0(
        data_contract_id: Identifier,
        name: &str,
        schema: Value,
        schema_defs: Option<&BTreeMap<String, Value>>,
        default_keeps_history: bool,
        default_mutability: bool,
        default_can_be_deleted: bool,
        full_validation: bool, // we don't need to validate if loaded from state
        validation_operations: &mut Vec<ProtocolValidationOperation>,
        platform_version: &PlatformVersion,
    ) -> Result<Self, ProtocolError> {
        // Create a full root JSON Schema from shorten contract document type schema
        let root_schema = DocumentType::enrich_with_base_schema(
            schema.clone(),
            schema_defs.map(|defs| Value::from(defs.clone())),
            platform_version,
        )?;

        #[cfg(not(feature = "validation"))]
        if full_validation {
            // TODO we are silently dropping this error when we shouldn't be
            // but returning this error causes tests to fail; investigate more.
            ProtocolError::CorruptedCodeExecution(
                "validation is not enabled but is being called on try_from_schema_v0".to_string(),
            );
        }

        #[cfg(feature = "validation")]
        let json_schema_validator = StatelessJsonSchemaLazyValidator::new();

        #[cfg(feature = "validation")]
        if full_validation {
            // Make sure a document type name is compliant
            if !name
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
                || name.is_empty()
                || name.len() > 64
            {
                return Err(ProtocolError::ConsensusError(Box::new(
                    InvalidDocumentTypeNameError::new(name.to_string()).into(),
                )));
            }

            // Validate document schema depth
            let mut result = validate_max_depth(&root_schema, platform_version)?;

            if !result.is_valid() {
                let error = result.errors.remove(0);

                let schema_size = result.into_data()?.size;

                validation_operations.push(
                    ProtocolValidationOperation::DocumentTypeSchemaValidationForSize(schema_size),
                );

                return Err(ProtocolError::ConsensusError(Box::new(error)));
            }

            let schema_size = result.into_data()?.size;

            validation_operations.push(
                ProtocolValidationOperation::DocumentTypeSchemaValidationForSize(schema_size),
            );

            // Make sure JSON Schema is compilable
            let root_json_schema = root_schema.try_to_validating_json().map_err(|e| {
                ProtocolError::ConsensusError(
                    ConsensusError::BasicError(BasicError::ValueError(e.into())).into(),
                )
            })?;

            // Validate against JSON Schema
            DOCUMENT_META_SCHEMA_V0
                .validate(&root_json_schema)
                .map_err(|mut errs| ConsensusError::from(errs.next().unwrap()))?;

            json_schema_validator.compile(&root_json_schema, platform_version)?;
        }

        // This has already been validated, but we leave the map_err here for consistency
        let schema_map = schema.to_map().map_err(|err| {
            consensus_or_protocol_data_contract_error(DataContractError::InvalidContractStructure(
                format!("document schema must be an object: {err}"),
            ))
        })?;

        // Do documents of this type keep history? (Overrides contract value)
        let documents_keep_history: bool =
            Value::inner_optional_bool_value(schema_map, DOCUMENTS_KEEP_HISTORY)
                .map_err(consensus_or_protocol_value_error)?
                .unwrap_or(default_keeps_history);

        // Are documents of this type mutable? (Overrides contract value)
        let documents_mutable: bool =
            Value::inner_optional_bool_value(schema_map, DOCUMENTS_MUTABLE)
                .map_err(consensus_or_protocol_value_error)?
                .unwrap_or(default_mutability);

        // Can documents of this type be deleted? (Overrides contract value)
        let documents_can_be_deleted: bool =
            Value::inner_optional_bool_value(schema_map, CAN_BE_DELETED)
                .map_err(consensus_or_protocol_value_error)?
                .unwrap_or(default_can_be_deleted);

        // Are documents of this type transferable?
        let documents_transferable_u8: u8 =
            Value::inner_optional_integer_value(schema_map, TRANSFERABLE)
                .map_err(consensus_or_protocol_value_error)?
                .unwrap_or_default();

        let documents_transferable = documents_transferable_u8.try_into()?;

        // What is the trade mode of these documents
        let documents_trade_mode_u8: u8 =
            Value::inner_optional_integer_value(schema_map, TRADE_MODE)
                .map_err(consensus_or_protocol_value_error)?
                .unwrap_or_default();

        let trade_mode = documents_trade_mode_u8.try_into()?;

        // What is the creation restriction mode of this document type?
        let documents_creation_restriction_mode_u8: u8 =
            Value::inner_optional_integer_value(schema_map, CREATION_RESTRICTION_MODE)
                .map_err(consensus_or_protocol_value_error)?
                .unwrap_or_default();

        let creation_restriction_mode = documents_creation_restriction_mode_u8.try_into()?;

        // Extract the properties
        let property_values = Value::inner_optional_index_map::<u64>(
            schema_map,
            property_names::PROPERTIES,
            property_names::POSITION,
        )
        .map_err(consensus_or_protocol_value_error)?
        .unwrap_or_default();

        #[cfg(feature = "validation")]
        if full_validation {
            validation_operations.push(
                ProtocolValidationOperation::DocumentTypeSchemaPropertyValidation(
                    property_values.values().len() as u64,
                ),
            );

            // We should validate that the positions are continuous
            for (pos, value) in property_values.values().enumerate() {
                if value.get_integer::<u32>(property_names::POSITION)? != pos as u32 {
                    return Err(ConsensusError::BasicError(
                        BasicError::MissingPositionsInDocumentTypePropertiesError(
                            MissingPositionsInDocumentTypePropertiesError::new(
                                pos as u32,
                                data_contract_id,
                                name.to_string(),
                            ),
                        ),
                    )
                    .into());
                }
            }
        }

        // Prepare internal data for efficient querying
        let mut flattened_document_properties: IndexMap<String, DocumentProperty> = IndexMap::new();
        let mut document_properties: IndexMap<String, DocumentProperty> = IndexMap::new();

        let required_fields = Value::inner_recursive_optional_array_of_strings(
            schema_map,
            "".to_string(),
            property_names::PROPERTIES,
            property_names::REQUIRED,
        );

        let transient_fields = Value::inner_recursive_optional_array_of_strings(
            schema_map,
            "".to_string(),
            property_names::PROPERTIES,
            property_names::TRANSIENT,
        );

        // Based on the property name, determine the type
        for (property_key, property_value) in property_values {
            // TODO: It's very inefficient. It must be done in one iteration and flattened properties
            //  must keep a reference? We even could keep only one collection
            insert_values(
                &mut flattened_document_properties,
                &required_fields,
                &transient_fields,
                None,
                property_key.clone(),
                property_value,
                &root_schema,
            )
            .map_err(consensus_or_protocol_data_contract_error)?;

            insert_values_nested(
                &mut document_properties,
                &required_fields,
                &transient_fields,
                property_key,
                property_value,
                &root_schema,
            )
            .map_err(consensus_or_protocol_data_contract_error)?;
        }

        // Initialize indices
        let index_values =
            Value::inner_optional_array_slice_value(schema_map, property_names::INDICES)
                .map_err(consensus_or_protocol_value_error)?;

        #[cfg(feature = "validation")]
        let mut index_names: HashSet<String> = HashSet::new();
        #[cfg(feature = "validation")]
        let mut unique_indices_count = 0;

        #[cfg(feature = "validation")]
        let mut last_non_contested_unique_index_name: Option<String> = None;

        #[cfg(feature = "validation")]
        let mut last_contested_unique_index_name: Option<String> = None;

        #[cfg(feature = "validation")]
        let mut contested_indices_count = 0;

        let indices: BTreeMap<String, Index> = index_values
            .map(|index_values| {
                index_values
                    .iter()
                    .map(|index_value| {
                        let index: Index = index_value
                            .to_map()
                            .map_err(consensus_or_protocol_value_error)?
                            .as_slice()
                            .try_into()
                            .map_err(consensus_or_protocol_data_contract_error)?;

                        #[cfg(feature = "validation")]
                        if full_validation {
                            validation_operations.push(
                                ProtocolValidationOperation::DocumentTypeSchemaIndexValidation(
                                    index.properties.len() as u64,
                                    index.unique,
                                ),
                            );

                            // Unique indices produces significant load on the system during state validation
                            // so we need to limit their number to prevent of spikes and DoS attacks
                            if index.unique {
                                unique_indices_count += 1;
                                if unique_indices_count
                                    > platform_version
                                        .dpp
                                        .validation
                                        .document_type
                                        .unique_index_limit
                                {
                                    return Err(ProtocolError::ConsensusError(Box::new(
                                        UniqueIndicesLimitReachedError::new(
                                            name.to_string(),
                                            platform_version
                                                .dpp
                                                .validation
                                                .document_type
                                                .unique_index_limit,
                                            false,
                                        )
                                        .into(),
                                    )));
                                }

                                if let Some(last_contested_unique_index_name) =
                                    last_contested_unique_index_name.as_ref()
                                {
                                    return Err(ProtocolError::ConsensusError(Box::new(
                                        ContestedUniqueIndexWithUniqueIndexError::new(
                                            name.to_string(),
                                            last_contested_unique_index_name.clone(),
                                            index.name,
                                        )
                                        .into(),
                                    )));
                                }

                                if index.contested_index.is_none() {
                                    last_non_contested_unique_index_name = Some(index.name.clone());
                                }
                            }

                            if index.contested_index.is_some() {
                                contested_indices_count += 1;
                                if contested_indices_count
                                    > platform_version
                                        .dpp
                                        .validation
                                        .document_type
                                        .contested_index_limit
                                {
                                    return Err(ProtocolError::ConsensusError(Box::new(
                                        UniqueIndicesLimitReachedError::new(
                                            name.to_string(),
                                            platform_version
                                                .dpp
                                                .validation
                                                .document_type
                                                .contested_index_limit,
                                            true,
                                        )
                                        .into(),
                                    )));
                                }

                                if let Some(last_unique_index_name) =
                                    last_non_contested_unique_index_name.as_ref()
                                {
                                    return Err(ProtocolError::ConsensusError(Box::new(
                                        ContestedUniqueIndexWithUniqueIndexError::new(
                                            name.to_string(),
                                            index.name,
                                            last_unique_index_name.clone(),
                                        )
                                        .into(),
                                    )));
                                }

                                if documents_mutable {
                                    return Err(ProtocolError::ConsensusError(Box::new(
                                        ContestedUniqueIndexOnMutableDocumentTypeError::new(
                                            name.to_string(),
                                            index.name,
                                        )
                                        .into(),
                                    )));
                                }

                                last_contested_unique_index_name = Some(index.name.clone());
                            }

                            // Index names must be unique for the document type
                            if !index_names.insert(index.name.to_owned()) {
                                return Err(ProtocolError::ConsensusError(Box::new(
                                    DuplicateIndexNameError::new(name.to_string(), index.name)
                                        .into(),
                                )));
                            }

                            // Validate indexed properties
                            index.properties.iter().try_for_each(|index_property| {
                                // Do not allow to index already indexed system properties
                                if NOT_ALLOWED_SYSTEM_PROPERTIES
                                    .contains(&index_property.name.as_str())
                                {
                                    return Err(ProtocolError::ConsensusError(Box::new(
                                        SystemPropertyIndexAlreadyPresentError::new(
                                            name.to_owned(),
                                            index.name.to_owned(),
                                            index_property.name.to_owned(),
                                        )
                                        .into(),
                                    )));
                                }

                                // Indexed property must be defined in user schema if it's not a system one
                                if !SYSTEM_PROPERTIES.contains(&index_property.name.as_str()) {
                                    let property_definition = flattened_document_properties
                                        .get(&index_property.name)
                                        .ok_or_else(|| {
                                            ProtocolError::ConsensusError(Box::new(
                                                UndefinedIndexPropertyError::new(
                                                    name.to_owned(),
                                                    index.name.to_owned(),
                                                    index_property.name.to_owned(),
                                                )
                                                .into(),
                                            ))
                                        })?;

                                    // Validate indexed property type
                                    match &property_definition.property_type {
                                        // Array and objects aren't supported for indexing yet
                                        DocumentPropertyType::Array(_)
                                        | DocumentPropertyType::Object(_)
                                        | DocumentPropertyType::VariableTypeArray(_) => {
                                            Err(ProtocolError::ConsensusError(Box::new(
                                                InvalidIndexPropertyTypeError::new(
                                                    name.to_owned(),
                                                    index.name.to_owned(),
                                                    index_property.name.to_owned(),
                                                    property_definition.property_type.name(),
                                                )
                                                .into(),
                                            )))
                                        }
                                        // Indexed byte array size must be limited
                                        DocumentPropertyType::ByteArray(sizes)
                                            if sizes.max_size.is_none()
                                                || sizes.max_size.unwrap()
                                                    > MAX_INDEXED_BYTE_ARRAY_PROPERTY_LENGTH =>
                                        {
                                            Err(ProtocolError::ConsensusError(Box::new(
                                                InvalidIndexedPropertyConstraintError::new(
                                                    name.to_owned(),
                                                    index.name.to_owned(),
                                                    index_property.name.to_owned(),
                                                    "maxItems".to_string(),
                                                    format!(
                                                        "should be less or equal {}",
                                                        MAX_INDEXED_BYTE_ARRAY_PROPERTY_LENGTH
                                                    ),
                                                )
                                                .into(),
                                            )))
                                        }
                                        // Indexed string length must be limited
                                        DocumentPropertyType::String(sizes)
                                            if sizes.max_length.is_none()
                                                || sizes.max_length.unwrap()
                                                    > MAX_INDEXED_STRING_PROPERTY_LENGTH =>
                                        {
                                            Err(ProtocolError::ConsensusError(Box::new(
                                                InvalidIndexedPropertyConstraintError::new(
                                                    name.to_owned(),
                                                    index.name.to_owned(),
                                                    index_property.name.to_owned(),
                                                    "maxLength".to_string(),
                                                    format!(
                                                        "should be less or equal {}",
                                                        MAX_INDEXED_STRING_PROPERTY_LENGTH
                                                    ),
                                                )
                                                .into(),
                                            )))
                                        }
                                        _ => Ok(()),
                                    }
                                } else {
                                    Ok(())
                                }
                            })?;
                        }

                        Ok((index.name.clone(), index))
                    })
                    .collect::<Result<BTreeMap<String, Index>, ProtocolError>>()
            })
            .transpose()?
            .unwrap_or_default();

        let index_structure =
            IndexLevel::try_from_indices(indices.values(), name, platform_version)?;

        // Collect binary and identifier properties
        let (identifier_paths, binary_paths) = DocumentType::find_identifier_and_binary_paths(
            &document_properties,
            &platform_version
                .dpp
                .contract_versions
                .document_type_versions,
        )?;

        let security_level_requirement = schema
            .get_optional_integer::<u8>(property_names::SECURITY_LEVEL_REQUIREMENT)
            .map_err(consensus_or_protocol_value_error)?
            .map(SecurityLevel::try_from)
            .transpose()?
            .unwrap_or(SecurityLevel::HIGH);

        let requires_identity_encryption_bounded_key = schema
            .get_optional_integer::<u8>(property_names::REQUIRES_IDENTITY_ENCRYPTION_BOUNDED_KEY)
            .map_err(consensus_or_protocol_value_error)?
            .map(StorageKeyRequirements::try_from)
            .transpose()?;

        let requires_identity_decryption_bounded_key = schema
            .get_optional_integer::<u8>(property_names::REQUIRES_IDENTITY_DECRYPTION_BOUNDED_KEY)
            .map_err(consensus_or_protocol_value_error)?
            .map(StorageKeyRequirements::try_from)
            .transpose()?;

        Ok(DocumentTypeV0 {
            name: String::from(name),
            schema,
            indices,
            index_structure,
            flattened_properties: flattened_document_properties,
            properties: document_properties,
            identifier_paths,
            binary_paths,
            required_fields,
            transient_fields,
            documents_keep_history,
            documents_mutable,
            documents_can_be_deleted,
            documents_transferable,
            trade_mode,
            creation_restriction_mode,
            data_contract_id,
            requires_identity_encryption_bounded_key,
            requires_identity_decryption_bounded_key,
            security_level_requirement,
            #[cfg(feature = "validation")]
            json_schema_validator,
        })
    }
}

fn insert_values(
    document_properties: &mut IndexMap<String, DocumentProperty>,
    known_required: &BTreeSet<String>,
    known_transient: &BTreeSet<String>,
    prefix: Option<String>,
    property_key: String,
    property_value: &Value,
    root_schema: &Value,
) -> Result<(), DataContractError> {
    let mut to_visit: Vec<(Option<String>, String, &Value)> =
        vec![(prefix, property_key, property_value)];

    while let Some((prefix, property_key, property_value)) = to_visit.pop() {
        let prefixed_property_key = match prefix {
            None => property_key,
            Some(prefix) => [prefix, property_key].join(".").to_owned(),
        };

        let mut inner_properties = property_value.to_btree_ref_string_map()?;

        if let Some(schema_ref) = inner_properties.get_optional_str(property_names::REF)? {
            let referenced_sub_schema = resolve_uri(root_schema, schema_ref)?;

            inner_properties = referenced_sub_schema.to_btree_ref_string_map()?
        }

        let type_value = inner_properties.get_str(property_names::TYPE)?;

        let is_required = known_required.contains(&prefixed_property_key);
        let is_transient = known_transient.contains(&prefixed_property_key);
        let field_type: DocumentPropertyType;

        match type_value {
            "array" => {
                // Only handling bytearrays for v1
                // Return an error if it is not a byte array
                field_type = match inner_properties.get_optional_bool(property_names::BYTE_ARRAY)? {
                    Some(inner_bool) => {
                        if inner_bool {
                            match inner_properties
                                .get_optional_str(property_names::CONTENT_MEDIA_TYPE)?
                            {
                                Some("application/x.dash.dpp.identifier") => {
                                    DocumentPropertyType::Identifier
                                }
                                Some(_) | None => {
                                    DocumentPropertyType::ByteArray(ByteArrayPropertySizes {
                                        min_size: inner_properties
                                            .get_optional_integer(property_names::MIN_ITEMS)?,
                                        max_size: inner_properties
                                            .get_optional_integer(property_names::MAX_ITEMS)?,
                                    })
                                }
                            }
                        } else {
                            return Err(DataContractError::InvalidContractStructure(
                                "byteArray should always be true if defined".to_string(),
                            ));
                        }
                    }
                    // TODO: Update when arrays are implemented
                    None => {
                        return Err(DataContractError::InvalidContractStructure(
                            "only byte arrays are supported now".to_string(),
                        ));
                    }
                };

                document_properties.insert(
                    prefixed_property_key,
                    DocumentProperty {
                        property_type: field_type,
                        required: is_required,
                        transient: is_transient,
                    },
                );
            }
            "object" => {
                if let Some(properties_as_value) = inner_properties.get(property_names::PROPERTIES)
                {
                    let properties =
                        properties_as_value
                            .as_map()
                            .ok_or(DataContractError::ValueWrongType(
                                "properties must be a map".to_string(),
                            ))?;

                    for (object_property_key, object_property_value) in properties.iter() {
                        let object_property_string = object_property_key
                            .as_text()
                            .ok_or(DataContractError::KeyWrongType(
                                "property key must be a string".to_string(),
                            ))?
                            .to_string();
                        to_visit.push((
                            Some(prefixed_property_key.clone()),
                            object_property_string,
                            object_property_value,
                        ));
                    }
                }
            }

            "string" => {
                field_type = DocumentPropertyType::String(StringPropertySizes {
                    min_length: inner_properties
                        .get_optional_integer(property_names::MIN_LENGTH)?,
                    max_length: inner_properties
                        .get_optional_integer(property_names::MAX_LENGTH)?,
                });
                document_properties.insert(
                    prefixed_property_key,
                    DocumentProperty {
                        property_type: field_type,
                        required: is_required,
                        transient: is_transient,
                    },
                );
            }

            _ => {
                field_type = DocumentPropertyType::try_from_name(type_value)?;

                document_properties.insert(
                    prefixed_property_key,
                    DocumentProperty {
                        property_type: field_type,
                        required: is_required,
                        transient: is_transient,
                    },
                );
            }
        }
    }

    Ok(())
}
fn insert_values_nested(
    document_properties: &mut IndexMap<String, DocumentProperty>,
    known_required: &BTreeSet<String>,
    known_transient: &BTreeSet<String>,
    property_key: String,
    property_value: &Value,
    root_schema: &Value,
) -> Result<(), DataContractError> {
    let mut inner_properties = property_value.to_btree_ref_string_map()?;

    if let Some(schema_ref) = inner_properties.get_optional_str(property_names::REF)? {
        let referenced_sub_schema = resolve_uri(root_schema, schema_ref)?;

        inner_properties = referenced_sub_schema.to_btree_ref_string_map()?;
    }

    let type_value = inner_properties.get_str(property_names::TYPE)?;

    let is_required = known_required.contains(&property_key);

    let is_transient = known_transient.contains(&property_key);

    let field_type = match type_value {
        "integer" => DocumentPropertyType::I64,
        "number" => DocumentPropertyType::F64,
        "string" => DocumentPropertyType::String(StringPropertySizes {
            min_length: inner_properties.get_optional_integer(property_names::MIN_LENGTH)?,
            max_length: inner_properties.get_optional_integer(property_names::MAX_LENGTH)?,
        }),
        "array" => {
            // Only handling bytearrays for v1
            // Return an error if it is not a byte array
            match inner_properties.get_optional_bool(property_names::BYTE_ARRAY)? {
                Some(inner_bool) => {
                    if inner_bool {
                        match inner_properties
                            .get_optional_str(property_names::CONTENT_MEDIA_TYPE)?
                        {
                            Some("application/x.dash.dpp.identifier") => {
                                DocumentPropertyType::Identifier
                            }
                            Some(_) | None => {
                                DocumentPropertyType::ByteArray(ByteArrayPropertySizes {
                                    min_size: inner_properties
                                        .get_optional_integer(property_names::MIN_ITEMS)?,
                                    max_size: inner_properties
                                        .get_optional_integer(property_names::MAX_ITEMS)?,
                                })
                            }
                        }
                    } else {
                        return Err(DataContractError::InvalidContractStructure(
                            "byteArray should always be true if defined".to_string(),
                        ));
                    }
                }
                // TODO: Contract indices and new encoding format don't support arrays
                //   but we still can use them as document fields with current cbor encoding
                //   This is a temporary workaround to bring back v0.22 behavior and should be
                //   replaced with a proper array support in future versions
                None => DocumentPropertyType::Array(ArrayItemType::Boolean),
            }
        }
        "object" => {
            let mut nested_properties = IndexMap::new();
            if let Some(properties_as_value) = inner_properties.get(property_names::PROPERTIES) {
                let properties =
                    properties_as_value
                        .as_map()
                        .ok_or(DataContractError::ValueWrongType(
                            "properties must be a map".to_string(),
                        ))?;

                let mut sorted_properties: Vec<_> = properties.iter().collect();

                sorted_properties.sort_by(|(_, value_1), (_, value_2)| {
                    let pos_1: u64 = value_1
                        .get_integer(property_names::POSITION)
                        .expect("expected a position");
                    let pos_2: u64 = value_2
                        .get_integer(property_names::POSITION)
                        .expect("expected a position");
                    pos_1.cmp(&pos_2)
                });

                // Create a new set with the prefix removed from the keys
                let stripped_required: BTreeSet<String> = known_required
                    .iter()
                    .filter_map(|key| {
                        if key.starts_with(&property_key) && key.len() > property_key.len() {
                            Some(key[property_key.len() + 1..].to_string())
                        } else {
                            None
                        }
                    })
                    .collect();

                let stripped_transient: BTreeSet<String> = known_transient
                    .iter()
                    .filter_map(|key| {
                        if key.starts_with(&property_key) && key.len() > property_key.len() {
                            Some(key[property_key.len() + 1..].to_string())
                        } else {
                            None
                        }
                    })
                    .collect();

                for (object_property_key, object_property_value) in properties.iter() {
                    let object_property_string = object_property_key
                        .as_text()
                        .ok_or(DataContractError::KeyWrongType(
                            "property key must be a string".to_string(),
                        ))?
                        .to_string();

                    insert_values_nested(
                        &mut nested_properties,
                        &stripped_required,
                        &stripped_transient,
                        object_property_string,
                        object_property_value,
                        root_schema,
                    )?;
                }
            }
            document_properties.insert(
                property_key,
                DocumentProperty {
                    property_type: DocumentPropertyType::Object(nested_properties),
                    required: is_required,
                    transient: is_transient,
                },
            );
            return Ok(());
        }
        _ => DocumentPropertyType::try_from_name(type_value)?,
    };

    document_properties.insert(
        property_key,
        DocumentProperty {
            property_type: field_type,
            required: is_required,
            transient: is_transient,
        },
    );

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use assert_matches::assert_matches;
    use platform_value::platform_value;

    mod document_type_name {
        use super::*;

        #[test]
        fn should_be_valid() {
            let platform_version = PlatformVersion::latest();

            let schema = platform_value!({
                "type": "object",
                "properties": {
                    "valid_name": {
                        "type": "string",
                        "position": 0
                    }
                },
                "additionalProperties": false
            });

            let _result = DocumentTypeV0::try_from_schema_v0(
                Identifier::new([1; 32]),
                "valid_name-a-b-123",
                schema,
                None,
                false,
                false,
                false,
                true,
                &mut vec![],
                platform_version,
            )
            .expect("should be valid");
        }

        #[test]
        fn should_no_be_empty() {
            let platform_version = PlatformVersion::latest();

            let schema = platform_value!({
                "type": "object",
                "properties": {
                    "valid_name": {
                        "type": "string",
                        "position": 0
                    }
                },
                "additionalProperties": false
            });

            let result = DocumentTypeV0::try_from_schema_v0(
                Identifier::new([1; 32]),
                "",
                schema,
                None,
                false,
                false,
                false,
                true,
                &mut vec![],
                platform_version,
            );

            assert_matches!(
                result,
                Err(ProtocolError::ConsensusError(boxed)) => {
                    assert_matches!(
                        boxed.as_ref(),
                        ConsensusError::BasicError(
                            BasicError::InvalidDocumentTypeNameError(InvalidDocumentTypeNameError { .. })
                        )
                    )
                }
            );
        }

        #[test]
        fn should_no_be_longer_than_64_chars() {
            let platform_version = PlatformVersion::latest();

            let schema = platform_value!({
                "type": "object",
                "properties": {
                    "valid_name": {
                        "type": "string",
                        "position": 0
                    }
                },
                "additionalProperties": false
            });

            let result = DocumentTypeV0::try_from_schema_v0(
                Identifier::new([1; 32]),
                &"a".repeat(65),
                schema,
                None,
                false,
                false,
                false,
                true,
                &mut vec![],
                platform_version,
            );

            assert_matches!(
                result,
                Err(ProtocolError::ConsensusError(boxed)) => {
                    assert_matches!(
                        boxed.as_ref(),
                        ConsensusError::BasicError(
                            BasicError::InvalidDocumentTypeNameError(InvalidDocumentTypeNameError { .. })
                        )
                    )
                }
            );
        }

        #[test]
        fn should_no_be_alphanumeric() {
            let platform_version = PlatformVersion::latest();

            let schema = platform_value!({
                "type": "object",
                "properties": {
                    "valid_name": {
                        "type": "string",
                        "position": 0
                    }
                },
                "additionalProperties": false
            });

            let result = DocumentTypeV0::try_from_schema_v0(
                Identifier::new([1; 32]),
                "invalid name",
                schema.clone(),
                None,
                false,
                false,
                false,
                true,
                &mut vec![],
                platform_version,
            );

            assert_matches!(
                result,
                Err(ProtocolError::ConsensusError(boxed)) => {
                    assert_matches!(
                        boxed.as_ref(),
                        ConsensusError::BasicError(
                            BasicError::InvalidDocumentTypeNameError(InvalidDocumentTypeNameError { .. })
                        )
                    )
                }
            );

            let result = DocumentTypeV0::try_from_schema_v0(
                Identifier::new([1; 32]),
                "invalid&name",
                schema,
                None,
                false,
                false,
                false,
                true,
                &mut vec![],
                platform_version,
            );

            assert_matches!(
                result,
                Err(ProtocolError::ConsensusError(boxed)) => {
                    assert_matches!(
                        boxed.as_ref(),
                        ConsensusError::BasicError(
                            BasicError::InvalidDocumentTypeNameError(InvalidDocumentTypeNameError { .. })
                        )
                    )
                }
            );
        }
    }
}
