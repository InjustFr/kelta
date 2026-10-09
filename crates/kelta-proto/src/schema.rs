//! JSON Schemas generated from the settings / extension structs (written by `xtask codegen` into
//! `schema/*.schema.json`). Subschemas are inlined; only recursive types (`TemplateNode`,
//! `Matcher`, `ActionDef`) keep `$defs` references.

use schemars::JsonSchema;
use schemars::generate::SchemaSettings;
use serde_json::Value;

use crate::ext::{PluginManifest, ToolDef, TriggerDef};
use crate::settings::{ProjectFile, Settings};

/// Public `$id` base (Taplo `#:schema` line).
pub const SCHEMA_BASE_URL: &str = "https://kelta.dev/schema/0.1";

fn generate<T: JsonSchema>(file: &str) -> Value {
    let generator = SchemaSettings::draft2020_12()
        .with(|s| {
            s.inline_subschemas = true;
        })
        .into_generator();
    let schema = generator.into_root_schema_for::<T>();
    let mut v = schema.to_value();
    if let Value::Object(map) = &mut v {
        map.insert("$id".into(), Value::String(format!("{SCHEMA_BASE_URL}/{file}")));
    }
    v
}

/// `schema/settings.schema.json`.
pub fn settings_schema() -> Value {
    generate::<Settings>("settings.schema.json")
}

/// `schema/project.schema.json`.
pub fn project_schema() -> Value {
    generate::<ProjectFile>("project.schema.json")
}

/// `schema/tool.schema.json`.
pub fn tool_schema() -> Value {
    generate::<ToolDef>("tool.schema.json")
}

/// `schema/trigger.schema.json`.
pub fn trigger_schema() -> Value {
    generate::<TriggerDef>("trigger.schema.json")
}

/// `schema/plugin-manifest.schema.json`.
pub fn plugin_manifest_schema() -> Value {
    generate::<PluginManifest>("plugin-manifest.schema.json")
}

/// `(file name, schema)` for every generated schema.
pub fn all() -> Vec<(&'static str, Value)> {
    vec![
        ("settings.schema.json", settings_schema()),
        ("project.schema.json", project_schema()),
        ("tool.schema.json", tool_schema()),
        ("trigger.schema.json", trigger_schema()),
        ("plugin-manifest.schema.json", plugin_manifest_schema()),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn schemas_have_annotations() {
        let s = settings_schema();
        let props = &s["properties"];
        assert_eq!(props["terminal"]["x-kelta-category"], "Terminal");
        assert_eq!(props["tools"]["x-kelta-merge"], "by_id");
        assert_eq!(props["accounts"]["x-kelta-scope"], serde_json::json!(["global"]));
        assert!(props["terminal"]["properties"]["max_live_views"].is_object());
        for (_, v) in all() {
            assert!(v.is_object());
        }
    }
}
