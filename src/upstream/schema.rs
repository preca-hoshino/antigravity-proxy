use serde_json::{Map, Value};
use std::collections::HashSet;

pub fn clean_json_schema_for_antigravity(schema: &Value, aggressive: bool) -> Value {
    match schema {
        Value::Bool(true) => serde_json::json!({ "type": "STRING" }),
        Value::Bool(false) => serde_json::json!({ "type": "NULL" }),
        Value::Object(map) => {
            if let Some(options) = map.get("anyOf").or_else(|| map.get("oneOf")) {
                if let Some(arr) = options.as_array() {
                    let best = arr
                        .iter()
                        .find(|opt| opt.get("type").and_then(|t| t.as_str()) == Some("object"))
                        .or_else(|| arr.first());
                    if let Some(b) = best {
                        return clean_json_schema_for_antigravity(b, aggressive);
                    }
                }
            }

            let unsupported: HashSet<&str> = [
                "additionalProperties",
                "$schema",
                "$id",
                "$comment",
                "$ref",
                "$defs",
                "definitions",
                "const",
                "contentMediaType",
                "contentEncoding",
                "if",
                "then",
                "else",
                "not",
                "patternProperties",
                "unevaluatedProperties",
                "unevaluatedItems",
                "dependentRequired",
                "dependentSchemas",
                "propertyNames",
                "minContains",
                "maxContains",
            ]
            .into_iter()
            .collect();

            let mut result = Map::new();

            let property_names: HashSet<String> = map
                .get("properties")
                .and_then(|p| p.as_object())
                .map(|p| p.keys().cloned().collect())
                .unwrap_or_default();

            for (k, v) in map {
                if unsupported.contains(k.as_str()) {
                    continue;
                }

                if k == "type" {
                    if let Some(t_str) = v.as_str() {
                        result.insert(k.clone(), Value::String(t_str.to_uppercase()));
                    }
                } else if k == "properties" {
                    if let Some(p_obj) = v.as_object() {
                        if !p_obj.is_empty() {
                            let mut cleaned_props = Map::new();
                            for (pk, pv) in p_obj {
                                cleaned_props.insert(pk.clone(), clean_json_schema_for_antigravity(pv, aggressive));
                            }
                            result.insert(k.clone(), Value::Object(cleaned_props));
                        } else {
                            result.insert(
                                k.clone(),
                                serde_json::json!({
                                    "_placeholder": {
                                        "type": "BOOLEAN",
                                        "description": "Technical placeholder to ensure non-empty schema"
                                    }
                                }),
                            );
                        }
                    }
                } else if k == "items" {
                    result.insert(k.clone(), clean_json_schema_for_antigravity(v, aggressive));
                } else if k == "required" {
                    if let Some(arr) = v.as_array() {
                        if !property_names.is_empty() {
                            let valid: Vec<Value> = arr
                                .iter()
                                .filter(|item| {
                                    item.as_str()
                                        .map(|s| property_names.contains(s))
                                        .unwrap_or(false)
                                })
                                .cloned()
                                .collect();
                            if !valid.is_empty() {
                                result.insert(k.clone(), Value::Array(valid));
                            }
                        } else if result.get("properties").and_then(|p| p.get("_placeholder")).is_some() {
                            result.insert(k.clone(), serde_json::json!(["_placeholder"]));
                        }
                    }
                } else if k == "description" && !aggressive {
                    result.insert(k.clone(), v.clone());
                } else if k == "enum" {
                    if let Some(arr) = v.as_array() {
                        let str_enums: Vec<Value> = arr
                            .iter()
                            .map(|val| match val {
                                Value::String(s) => Value::String(s.clone()),
                                other => Value::String(other.to_string()),
                            })
                            .collect();
                        result.insert(k.clone(), Value::Array(str_enums));
                    }
                } else if k == "format" || k == "default" || k == "examples" {
                    result.insert(k.clone(), v.clone());
                }
            }

            if result.get("type").and_then(|t| t.as_str()) == Some("ARRAY") && !result.contains_key("items") {
                result.insert("items".to_string(), serde_json::json!({ "type": "STRING" }));
            }

            if !result.contains_key("type") && map.contains_key("properties") {
                result.insert("type".to_string(), Value::String("OBJECT".to_string()));
            }

            Value::Object(result)
        }
        _ => schema.clone(),
    }
}
