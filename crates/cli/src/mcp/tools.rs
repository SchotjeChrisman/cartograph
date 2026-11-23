//! The six tools: their schemas, and the translation of a call's arguments into a request.

use graph::ComponentId;
use query::{
    DEFAULT_BUDGET, DEFAULT_DEPTH, DEFAULT_IMPACT_DEPTH, DEFAULT_PATH_DEPTH, Page, Relation,
    Request,
};
use serde_json::{Map, Value, json};

/// A call whose arguments do not fit the tool. Reported to the model so it can correct them.
#[derive(Debug, PartialEq, Eq)]
pub struct ArgumentError(pub String);

fn budget_property() -> Value {
    json!({
        "type": "integer", "minimum": 1, "default": DEFAULT_BUDGET,
        "description": "Approximate size of the answer in tokens; the answer is cut there and ends with the command that shows more."
    })
}

fn after_property() -> Value {
    json!({
        "type": "integer", "minimum": 0, "default": 0,
        "description": "Skip this many ranked results: the cursor from a previous answer's footer."
    })
}

fn json_property() -> Value {
    json!({
        "type": "boolean", "default": false,
        "description": "Return the full structured result as JSON instead of dense text."
    })
}

fn depth_property(default: u32, what: &str) -> Value {
    json!({ "type": "integer", "minimum": 1, "default": default, "description": what })
}

fn component_property(what: &str) -> Value {
    json!({ "type": "integer", "description": what })
}

fn schema(properties: Value, required: &[&str], paged: bool) -> Value {
    let mut properties = properties.as_object().cloned().unwrap_or_default();
    properties.insert("budget".into(), budget_property());
    if paged {
        properties.insert("after".into(), after_property());
    }
    properties.insert("json".into(), json_property());
    json!({ "type": "object", "properties": properties, "required": required })
}

fn tool(name: &str, description: &str, input_schema: Value) -> Value {
    json!({ "name": name, "description": description, "inputSchema": input_schema })
}

pub fn definitions() -> Value {
    json!([
        tool(
            "overview",
            "Components of the codebase with their descriptions and the most connected symbols. Start here in an unfamiliar area.",
            schema(
                json!({ "component": component_property("Show the children of this component id instead of the top level.") }),
                &[],
                true,
            ),
        ),
        tool(
            "find",
            "Resolve a name, qualified name, path:line or words to ranked candidates. Each has a short @id to pass to the other tools.",
            schema(
                json!({ "text": { "type": "string", "description": "What to look for." } }),
                &["text"],
                true,
            ),
        ),
        tool(
            "explore",
            "The neighbourhood of one node: callers, callees or contents, to a depth.",
            schema(
                json!({
                    "target": { "type": "string", "description": "A name, qualified name, path:line or @id." },
                    "relation": { "type": "string", "enum": ["all", "callers", "callees", "contains"], "default": "all" },
                    "depth": depth_property(DEFAULT_DEPTH, "How many hops to follow."),
                    "component": component_property("Keep only nodes of this component id."),
                }),
                &["target"],
                true,
            ),
        ),
        tool(
            "path",
            "The shortest path from one node to another.",
            schema(
                json!({
                    "from": { "type": "string", "description": "A name, qualified name, path:line or @id." },
                    "to": { "type": "string", "description": "A name, qualified name, path:line or @id." },
                    "depth": depth_property(DEFAULT_PATH_DEPTH, "Longest path to look for."),
                }),
                &["from", "to"],
                false,
            ),
        ),
        tool(
            "impact",
            "What reaches a node, transitively: what breaks if it changes.",
            schema(
                json!({
                    "target": { "type": "string", "description": "A name, qualified name, path:line or @id." },
                    "depth": depth_property(DEFAULT_IMPACT_DEPTH, "How many hops of callers to follow."),
                }),
                &["target"],
                true,
            ),
        ),
        tool(
            "coverage",
            "What the graph parsed and what it did not, with the reason per file and how many calls resolved. Read or grep unparsed files directly.",
            schema(json!({}), &[], true),
        ),
    ])
}

/// The arguments of one call.
pub struct Arguments<'a>(&'a Map<String, Value>);

impl<'a> Arguments<'a> {
    pub fn new(value: &'a Value) -> Result<Self, ArgumentError> {
        match value {
            Value::Object(map) => Ok(Arguments(map)),
            _ => Err(ArgumentError("arguments must be an object".into())),
        }
    }

    pub fn wants_json(&self) -> Result<bool, ArgumentError> {
        match self.0.get("json") {
            None => Ok(false),
            Some(value) => value
                .as_bool()
                .ok_or_else(|| ArgumentError("`json` must be a boolean".into())),
        }
    }

    fn text(&self, name: &str) -> Result<String, ArgumentError> {
        self.0
            .get(name)
            .and_then(Value::as_str)
            .filter(|text| !text.is_empty())
            .map(str::to_string)
            .ok_or_else(|| {
                ArgumentError(format!(
                    "`{name}` is required and must be a non-empty string"
                ))
            })
    }

    fn whole(&self, name: &str) -> Result<Option<i64>, ArgumentError> {
        match self.0.get(name) {
            None | Some(Value::Null) => Ok(None),
            Some(value) => value
                .as_i64()
                .filter(|number| *number >= 0)
                .map(Some)
                .ok_or_else(|| ArgumentError(format!("`{name}` must be a non-negative integer"))),
        }
    }

    fn depth(&self, default: u32) -> Result<u32, ArgumentError> {
        match self.whole("depth")? {
            None => Ok(default),
            Some(depth) => {
                u32::try_from(depth).map_err(|_| ArgumentError("`depth` is too large".into()))
            }
        }
    }

    fn budget(&self) -> Result<usize, ArgumentError> {
        Ok(self
            .whole("budget")?
            .map_or(DEFAULT_BUDGET, |budget| budget as usize))
    }

    fn page(&self) -> Result<Page, ArgumentError> {
        Ok(Page {
            budget: self.budget()?,
            after: self.whole("after")?.unwrap_or(0) as usize,
        })
    }

    fn component(&self) -> Result<Option<ComponentId>, ArgumentError> {
        Ok(self.whole("component")?.map(ComponentId))
    }

    fn relation(&self) -> Result<Relation, ArgumentError> {
        match self.0.get("relation").and_then(Value::as_str) {
            None | Some("all") => Ok(Relation::All),
            Some("callers") => Ok(Relation::Callers),
            Some("callees") => Ok(Relation::Callees),
            Some("contains") => Ok(Relation::Contains),
            Some(other) => Err(ArgumentError(format!(
                "`relation` must be all, callers, callees or contains, not `{other}`"
            ))),
        }
    }
}

/// The request for tool `name`, or `None` when there is no such tool.
pub fn request(name: &str, arguments: &Arguments) -> Option<Result<Request, ArgumentError>> {
    Some(match name {
        "overview" => build_overview(arguments),
        "find" => build_find(arguments),
        "explore" => build_explore(arguments),
        "path" => build_path(arguments),
        "impact" => build_impact(arguments),
        "coverage" => arguments
            .page()
            .map(|page| Request::Coverage(query::Coverage { page })),
        _ => return None,
    })
}

fn build_overview(arguments: &Arguments) -> Result<Request, ArgumentError> {
    Ok(Request::Overview(query::Overview {
        component: arguments.component()?,
        page: arguments.page()?,
    }))
}

fn build_find(arguments: &Arguments) -> Result<Request, ArgumentError> {
    Ok(Request::Find(query::Find {
        text: arguments.text("text")?,
        page: arguments.page()?,
    }))
}

fn build_explore(arguments: &Arguments) -> Result<Request, ArgumentError> {
    Ok(Request::Explore(query::Explore {
        target: arguments.text("target")?,
        relation: arguments.relation()?,
        depth: arguments.depth(DEFAULT_DEPTH)?,
        component: arguments.component()?,
        page: arguments.page()?,
    }))
}

fn build_path(arguments: &Arguments) -> Result<Request, ArgumentError> {
    Ok(Request::Path(query::PathRequest {
        from: arguments.text("from")?,
        to: arguments.text("to")?,
        max_depth: arguments.depth(DEFAULT_PATH_DEPTH)?,
        budget: arguments.budget()?,
    }))
}

fn build_impact(arguments: &Arguments) -> Result<Request, ArgumentError> {
    Ok(Request::Impact(query::Impact {
        target: arguments.text("target")?,
        depth: arguments.depth(DEFAULT_IMPACT_DEPTH)?,
        page: arguments.page()?,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    const TOOL_NAMES: [&str; 6] = ["overview", "find", "explore", "path", "impact", "coverage"];

    #[test]
    fn every_tool_has_a_definition_and_a_request_builder() {
        let defined: Vec<String> = definitions()
            .as_array()
            .unwrap()
            .iter()
            .map(|tool| tool["name"].as_str().unwrap().to_string())
            .collect();
        assert_eq!(defined, TOOL_NAMES);
        let args = json!({ "text": "x", "target": "x", "from": "x", "to": "x" });
        let arguments = Arguments::new(&args).unwrap();
        for name in TOOL_NAMES {
            assert!(request(name, &arguments).unwrap().is_ok(), "{name}");
        }
    }

    #[test]
    fn budget_and_cursor_reach_the_request() {
        let args = json!({ "text": "open", "budget": 300, "after": 4 });
        let built = request("find", &Arguments::new(&args).unwrap())
            .unwrap()
            .unwrap();
        let Request::Find(find) = built else {
            panic!("expected find")
        };
        assert_eq!(
            find.page,
            Page {
                budget: 300,
                after: 4
            }
        );
    }

    #[test]
    fn bad_arguments_say_what_is_wrong() {
        let args = json!({ "target": "x", "relation": "sideways" });
        let error = request("explore", &Arguments::new(&args).unwrap())
            .unwrap()
            .unwrap_err();
        assert!(error.0.contains("relation"));
        let args = json!({});
        let error = request("find", &Arguments::new(&args).unwrap())
            .unwrap()
            .unwrap_err();
        assert!(error.0.contains("text"));
        assert!(request("nope", &Arguments::new(&args).unwrap()).is_none());
    }
}
