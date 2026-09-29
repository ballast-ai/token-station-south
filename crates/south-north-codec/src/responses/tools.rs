use super::{invalid, required_str};
use crate::CodecError;
use serde_json::{Value, json};
use std::collections::BTreeMap;
use token_station_protocol::{Extensions, ToolDef};
pub(super) const LOCAL_SHELL: &str = "__token_station_responses_local_shell";
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum ToolKind {
    Function,
    Custom,
    Search,
    Shell,
    Namespace { namespace: String, name: String },
}
pub(super) type ToolMap = BTreeMap<String, ToolKind>;
pub(super) fn namespace_name(
    namespace: &str,
    name: &str,
    path: &str,
) -> Result<String, CodecError> {
    let valid = |s: &str, limit| {
        !s.is_empty()
            && s.len() <= limit
            && s.bytes().all(|b| b.is_ascii_alphanumeric() || matches!(b, b'_' | b'-'))
    };
    if !valid(namespace, 64) || !valid(name, 128) {
        return Err(invalid(path, "invalid namespace or function name"));
    }
    let flat = if namespace.ends_with("__") {
        format!("{namespace}{name}")
    } else {
        format!("{namespace}__{name}")
    };
    if flat.len() > 128 {
        return Err(invalid(path, "flattened tool name exceeds 128 bytes"));
    }
    Ok(flat)
}
pub(super) fn tool_definitions(value: &Value) -> Result<(Vec<ToolDef>, Extensions), CodecError> {
    let (tools, extensions, _) = parse_tools(value)?;
    Ok((tools, extensions))
}
pub(super) fn restore_map(value: &Value) -> Result<ToolMap, CodecError> {
    let (_, _, map) = parse_tools(value)?;
    Ok(map)
}
fn parse_tools(value: &Value) -> Result<(Vec<ToolDef>, Extensions, ToolMap), CodecError> {
    if value.is_null() {
        return Ok((Vec::new(), Extensions::new(), ToolMap::new()));
    }
    let values = value.as_array().ok_or_else(|| invalid("tools", "tools must be an array"))?;
    let mut tools = Vec::new();
    let mut extensions = Extensions::new();
    let mut map = ToolMap::new();
    let mut strict = json!({});
    let mut namespaces = json!({});
    let mut disabled = Vec::new();
    for (i, tool) in values.iter().enumerate() {
        let path = format!("tools[{i}]");
        let kind = required_str(&tool["type"], &format!("{path}.type"))?;
        if kind == "namespace" {
            let namespace = required_str(&tool["name"], &format!("{path}.name"))?;
            let children = tool["tools"]
                .as_array()
                .ok_or_else(|| invalid(format!("{path}.tools"), "must be an array"))?;
            for (j, child) in children.iter().enumerate() {
                let field = format!("{path}.tools[{j}]");
                if child["type"] != "function" {
                    return Err(super::unsupported(format!("{field}.type"), "function"));
                }
                let name = required_str(&child["name"], &format!("{field}.name"))?;
                let flat = namespace_name(namespace, name, &field)?;
                let mut definition = function_definition(child, &flat, &field)?;
                if let Some(description) = tool.get("description").filter(|v| !v.is_null()) {
                    let description = required_str(description, &format!("{path}.description"))?;
                    definition.description = Some(definition.description.map_or_else(
                        || description.to_owned(),
                        |child| format!("{description}\n\n{child}"),
                    ));
                }
                insert(
                    &mut tools,
                    &mut map,
                    definition,
                    ToolKind::Namespace { namespace: namespace.into(), name: name.into() },
                    &field,
                )?;
                capture_strict(child, &flat, &field, &mut strict)?;
                namespaces[&flat] = json!({"namespace":namespace,"name":name});
            }
            continue;
        }
        let(name,parameters,description,restore)=match kind{
            "function"=>{let name=required_str(&tool["name"],&format!("{path}.name"))?;let def=function_definition(tool,name,&path)?;capture_strict(tool,name,&path,&mut strict)?;(def.name,def.parameters,def.description,ToolKind::Function)},
            "custom"=>{let name=tool.get("name").map(|v|required_str(v,&format!("{path}.name"))).transpose()?.unwrap_or("custom");(name.into(),json!({"type":"object","properties":{"input":{"type":"string","description":"Raw string input for the original custom tool. Preserve formatting exactly and follow the original tool definition embedded in the description."}},"required":["input"]}),Some(format!("Original tool definition:\n```json\n{tool}\n```")),ToolKind::Custom)},
            "tool_search"=>("tool_search".into(),json!({"type":"object","properties":{"query":{"type":"string","description":"Search query for tools or connectors to load."},"limit":{"type":"integer","description":"Maximum number of tool groups to return."}},"required":["query"]}),Some("Search and load Codex tools, plugins, connectors, and MCP namespaces for the current task.".into()),ToolKind::Search),
            "local_shell"=>(LOCAL_SHELL.into(),json!({"type":"object","properties":{"action":{"type":"object","properties":{"type":{"const":"exec"},"command":{"type":"array","items":{"type":"string"},"minItems":1},"env":{"type":"object","additionalProperties":{"type":"string"}},"timeout_ms":{"type":"integer","minimum":1},"user":{"type":"string"},"working_directory":{"type":"string"}},"required":["type","command"],"additionalProperties":false}},"required":["action"],"additionalProperties":false}),Some("Execute one argv command in the Codex client's local shell.".into()),ToolKind::Shell),
            "web_search" if tool["external_web_access"]==false=>{disabled.push(tool.clone());continue},
            _=>return Err(unsupported_tool(kind, &path)),
        };
        insert(&mut tools, &mut map, ToolDef { name, description, parameters }, restore, &path)?;
    }
    if strict.as_object().is_some_and(|o| !o.is_empty()) {
        extensions.insert("responses_tool_strict".into(), strict);
    }
    if namespaces.as_object().is_some_and(|o| !o.is_empty()) {
        extensions.insert("responses_tool_namespaces".into(), namespaces);
    }
    if !disabled.is_empty() {
        extensions.insert("responses_disabled_provider_tools".into(), json!(disabled));
    }
    Ok((tools, extensions, map))
}
fn function_definition(tool: &Value, name: &str, path: &str) -> Result<ToolDef, CodecError> {
    if name.is_empty() {
        return Err(invalid(format!("{path}.name"), "must not be empty"));
    }
    let parameters = tool.get("parameters").cloned().unwrap_or_else(|| json!({}));
    if !parameters.is_object() {
        return Err(invalid(format!("{path}.parameters"), "must be an object"));
    }
    let description = tool
        .get("description")
        .filter(|v| !v.is_null())
        .map(|v| required_str(v, &format!("{path}.description")).map(str::to_owned))
        .transpose()?;
    Ok(ToolDef { name: name.into(), description, parameters })
}
fn capture_strict(
    tool: &Value,
    name: &str,
    path: &str,
    strict: &mut Value,
) -> Result<(), CodecError> {
    if let Some(value) = tool.get("strict").filter(|v| !v.is_null()) {
        strict[name] = json!(
            value
                .as_bool()
                .ok_or_else(|| invalid(format!("{path}.strict"), "must be a boolean"))?
        );
    }
    Ok(())
}
fn insert(
    tools: &mut Vec<ToolDef>,
    map: &mut ToolMap,
    def: ToolDef,
    kind: ToolKind,
    path: &str,
) -> Result<(), CodecError> {
    if map.insert(def.name.clone(), kind).is_some() {
        return Err(invalid(format!("{path}.name"), "tool names collide after flattening"));
    }
    tools.push(def);
    Ok(())
}
pub(super) fn tool_item(
    id: &str,
    name: &str,
    args: &str,
    status: &str,
    map: &ToolMap,
) -> Result<Value, CodecError> {
    let kind = map
        .get(name)
        .cloned()
        .unwrap_or_else(|| if name == LOCAL_SHELL { ToolKind::Shell } else { ToolKind::Function });
    Ok(match kind {
        ToolKind::Custom => {
            let input = custom_input(args);
            json!({"type":"custom_tool_call","id":format!("ctc_{id}"),"call_id":id,"name":name,"input":input,"status":status})
        }
        ToolKind::Search => {
            let parsed =
                serde_json::from_str::<Value>(args).ok().filter(Value::is_object).unwrap_or_else(
                    || if args.trim().is_empty() { json!({}) } else { json!({"query":args}) },
                );
            json!({"type":"tool_search_call","id":format!("fc_{id}"),"call_id":id,"execution":"client","arguments":parsed,"status":status})
        }
        ToolKind::Shell => {
            let parsed = serde_json::from_str::<Value>(args).map_err(|_| {
                invalid(
                    "tool_calls.arguments",
                    "local_shell returned invalid arguments: expected JSON",
                )
            })?;
            if !parsed["action"].is_object() {
                return Err(invalid("tool_calls.arguments.action", "must be an object"));
            }
            json!({"type":"local_shell_call","id":format!("ls_{id}"),"call_id":id,"action":parsed["action"],"status":status})
        }
        ToolKind::Namespace { namespace, name } => {
            json!({"type":"function_call","id":format!("fc_{id}"),"call_id":id,"name":name,"namespace":namespace,"arguments":args,"status":status})
        }
        ToolKind::Function => {
            json!({"type":"function_call","id":format!("fc_{id}"),"call_id":id,"name":name,"arguments":args,"status":status})
        }
    })
}
pub(super) fn custom_input(args: &str) -> Value {
    if args.trim().is_empty() {
        return json!("");
    }
    serde_json::from_str::<Value>(args)
        .ok()
        .and_then(|mut v| v.as_object_mut().and_then(|o| o.remove("input")))
        .unwrap_or_else(|| json!(args))
}

fn unsupported_tool(kind: &str, path: &str) -> CodecError {
    // These fixed protocol names are public diagnostics used by host receipts.
    // Arbitrary client strings must never be reflected into error messages.
    let safe_name = match kind {
        "web_search" => "web_search",
        "web_search_preview" => "web_search_preview",
        "file_search" => "file_search",
        "code_interpreter" => "code_interpreter",
        "image_generation" => "image_generation",
        "computer_use_preview" => "computer_use_preview",
        "mcp" => "mcp",
        _ => "an unsupported value",
    };
    CodecError::unknown_value(format!("{path}.type"), safe_name, "a supported Responses tool")
}
