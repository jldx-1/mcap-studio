//! JSON and embedded Protobuf message decoding.
use anyhow::{bail, ensure, Context, Result};
use base64::{engine::general_purpose::STANDARD, Engine};
use prost_reflect::{DescriptorPool, DynamicMessage, MessageDescriptor, ReflectMessage};
use serde_json::{json, Map, Value};
use std::collections::HashMap;

use super::finite_number;

#[derive(Default)]
pub(super) struct Decoder {
    descriptors: HashMap<u16, MessageDescriptor>,
}

impl Decoder {
    pub(super) fn decode(&mut self, message: &mcap::Message<'_>) -> Result<Value> {
        self.decode_inner(message, false).map(|(record, _)| record)
    }

    /// Keep camera bytes binary while building previews. The public record and
    /// translation paths continue to expose the usual base64 JSON representation.
    pub(super) fn decode_for_index(
        &mut self,
        message: &mcap::Message<'_>,
    ) -> Result<(Value, Option<Vec<u8>>)> {
        self.decode_inner(message, true)
    }

    fn decode_inner(
        &mut self,
        message: &mcap::Message<'_>,
        separate_image: bool,
    ) -> Result<(Value, Option<Vec<u8>>)> {
        let channel = &message.channel;
        match channel.message_encoding.as_str() {
            "json" | "application/json" => Ok((serde_json::from_slice(&message.data)?, None)),
            "protobuf" => {
                let schema = channel
                    .schema
                    .as_ref()
                    .context("Protobuf channel has no schema")?;
                ensure!(
                    schema.encoding == "protobuf",
                    "Unsupported Protobuf schema encoding {}",
                    schema.encoding
                );
                if let std::collections::hash_map::Entry::Vacant(entry) =
                    self.descriptors.entry(schema.id)
                {
                    let pool = DescriptorPool::decode(schema.data.as_ref())
                        .context("Invalid embedded Protobuf descriptor set")?;
                    let descriptor = pool
                        .get_message_by_name(schema.name.trim_start_matches('.'))
                        .with_context(|| format!("Schema does not define {}", schema.name))?;
                    entry.insert(descriptor);
                }
                let message = DynamicMessage::decode(
                    self.descriptors[&schema.id].clone(),
                    message.data.as_ref(),
                )?;
                if separate_image
                    && ["width", "format", "encoding", "pixel_format", "codec"]
                        .iter()
                        .any(|name| message.descriptor().get_field_by_name(name).is_some())
                {
                    if let Some(field) = message.descriptor().get_field_by_name("data") {
                        if field.containing_oneof().is_none() || message.has_field(&field) {
                            if let prost_reflect::Value::Bytes(bytes) =
                                message.get_field(&field).as_ref()
                            {
                                let mut record = message_json_skipping(&message, 0, Some("data"))?;
                                record["data"] = Value::Null;
                                if super::is_camera(&record) {
                                    return Ok((record, Some(bytes.to_vec())));
                                }
                            }
                        }
                    }
                }
                Ok((message_json(&message, 0)?, None))
            }
            encoding => {
                let mut record = json!({"format":encoding,"data":null});
                if super::video::compressed_format(&record).is_some() {
                    if separate_image {
                        return Ok((record, Some(message.data.to_vec())));
                    }
                    record["data"] = json!({"$bytes_base64":STANDARD.encode(&message.data)});
                    return Ok((record, None));
                }
                bail!("Unsupported message encoding {encoding}; native decoding supports JSON, Protobuf, and compressed H.264/JPEG/PNG camera packets")
            }
        }
    }
}

pub(super) fn message_json(message: &DynamicMessage, depth: usize) -> Result<Value> {
    message_json_skipping(message, depth, None)
}

fn message_json_skipping(
    message: &DynamicMessage,
    depth: usize,
    skip: Option<&str>,
) -> Result<Value> {
    ensure!(depth < 64, "Protobuf nesting exceeds 64 levels");
    let mut record = Map::new();
    for field in message.descriptor().fields() {
        if skip == Some(field.name()) {
            continue;
        }
        if field.containing_oneof().is_some() && !message.has_field(&field) {
            continue;
        }
        // Absent messages are omitted; recursing into default recursive messages
        // would otherwise never terminate. Scalar defaults and empty arrays stay.
        if matches!(field.kind(), prost_reflect::Kind::Message(_))
            && !field.is_list()
            && !field.is_map()
            && !message.has_field(&field)
        {
            continue;
        }
        record.insert(
            field.name().into(),
            reflected_json(message.get_field(&field).as_ref(), depth + 1)?,
        );
    }
    Ok(Value::Object(record))
}

pub(super) fn reflected_json(value: &prost_reflect::Value, depth: usize) -> Result<Value> {
    use prost_reflect::Value as P;
    Ok(match value {
        P::Bool(v) => json!(v),
        P::I32(v) => json!(v),
        P::I64(v) => json!(v),
        P::U32(v) => json!(v),
        P::U64(v) => json!(v),
        P::F32(v) => finite_number(*v as f64)?,
        P::F64(v) => finite_number(*v)?,
        P::String(v) => json!(v),
        P::EnumNumber(v) => json!(v),
        P::Bytes(v) => json!({"$bytes_base64": STANDARD.encode(v)}),
        P::Message(v) => message_json(v, depth)?,
        P::List(v) => Value::Array(
            v.iter()
                .map(|item| reflected_json(item, depth))
                .collect::<Result<_>>()?,
        ),
        P::Map(v) => {
            let mut output = Map::new();
            for (key, item) in v {
                let key = match key {
                    prost_reflect::MapKey::Bool(v) => v.to_string(),
                    prost_reflect::MapKey::I32(v) => v.to_string(),
                    prost_reflect::MapKey::I64(v) => v.to_string(),
                    prost_reflect::MapKey::U32(v) => v.to_string(),
                    prost_reflect::MapKey::U64(v) => v.to_string(),
                    prost_reflect::MapKey::String(v) => v.clone(),
                };
                output.insert(key, reflected_json(item, depth)?);
            }
            Value::Object(output)
        }
    })
}
