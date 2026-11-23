//! What a caller asks for. The CLI and the MCP tools build these; each knows the command line
//! that repeats it, which is what the footer hands back to the reader.
//!
//! Command grammar, one subcommand per tool:
//! `cartograph overview [--component ID]`, `find TEXT`, `explore TARGET [--callers|--callees|--contains]
//! [--depth N] [--component ID]`, `path FROM TO [--depth N]`, `impact TARGET [--depth N]`,
//! `coverage`; all take `[--after N] [--budget N]` except `path`, which takes only `--budget`.

use graph::ComponentId;
use serde::Serialize;

pub const DEFAULT_BUDGET: usize = 2000;
pub const DEFAULT_DEPTH: u32 = 1;
pub const DEFAULT_PATH_DEPTH: u32 = 8;
/// Impact follows callers transitively, unlike `explore`.
pub const DEFAULT_IMPACT_DEPTH: u32 = 5;

/// Which slice of a ranked list to show.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct Page {
    /// Approximate size of the answer in tokens.
    pub budget: usize,
    /// How many ranked items to skip: the cursor a previous footer printed.
    pub after: usize,
}

impl Default for Page {
    fn default() -> Self {
        Page {
            budget: DEFAULT_BUDGET,
            after: 0,
        }
    }
}

impl Page {
    fn flags(self) -> String {
        let mut flags = String::new();
        if self.after > 0 {
            flags.push_str(&format!(" --after {}", self.after));
        }
        if self.budget != DEFAULT_BUDGET {
            flags.push_str(&format!(" --budget {}", self.budget));
        }
        flags
    }
}

/// Which edges `explore` follows from the target.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Relation {
    Callers,
    Callees,
    Contains,
    /// Every edge, either way.
    All,
}

impl Relation {
    fn flag(self) -> &'static str {
        match self {
            Relation::Callers => " --callers",
            Relation::Callees => " --callees",
            Relation::Contains => " --contains",
            Relation::All => "",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Overview {
    /// Show the children of this component; `None` shows the top level.
    pub component: Option<ComponentId>,
    pub page: Page,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Find {
    pub text: String,
    pub page: Page,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Explore {
    /// A name, qualified name, `path:line` or `@id`.
    pub target: String,
    pub relation: Relation,
    pub depth: u32,
    /// Keep only nodes of this component.
    pub component: Option<ComponentId>,
    pub page: Page,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct PathRequest {
    pub from: String,
    pub to: String,
    pub max_depth: u32,
    pub budget: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Impact {
    pub target: String,
    pub depth: u32,
    pub page: Page,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Coverage {
    pub page: Page,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "tool", rename_all = "snake_case")]
pub enum Request {
    Overview(Overview),
    Find(Find),
    Explore(Explore),
    Path(PathRequest),
    Impact(Impact),
    Coverage(Coverage),
}

impl Overview {
    pub fn command(&self) -> String {
        let component = self
            .component
            .map(|id| format!(" --component {}", id.0))
            .unwrap_or_default();
        format!("cartograph overview{component}{}", self.page.flags())
    }
}

impl Find {
    pub fn command(&self) -> String {
        format!("cartograph find {}{}", quote(&self.text), self.page.flags())
    }
}

impl Explore {
    pub fn command(&self) -> String {
        let depth = if self.depth == DEFAULT_DEPTH {
            String::new()
        } else {
            format!(" --depth {}", self.depth)
        };
        let component = self
            .component
            .map(|id| format!(" --component {}", id.0))
            .unwrap_or_default();
        format!(
            "cartograph explore {}{}{depth}{component}{}",
            quote(&self.target),
            self.relation.flag(),
            self.page.flags()
        )
    }
}

impl PathRequest {
    pub fn command(&self) -> String {
        let depth = if self.max_depth == DEFAULT_PATH_DEPTH {
            String::new()
        } else {
            format!(" --depth {}", self.max_depth)
        };
        let budget = if self.budget == DEFAULT_BUDGET {
            String::new()
        } else {
            format!(" --budget {}", self.budget)
        };
        format!(
            "cartograph path {} {}{depth}{budget}",
            quote(&self.from),
            quote(&self.to)
        )
    }
}

impl Impact {
    pub fn command(&self) -> String {
        let depth = if self.depth == DEFAULT_IMPACT_DEPTH {
            String::new()
        } else {
            format!(" --depth {}", self.depth)
        };
        format!(
            "cartograph impact {}{depth}{}",
            quote(&self.target),
            self.page.flags()
        )
    }
}

impl Coverage {
    pub fn command(&self) -> String {
        format!("cartograph coverage{}", self.page.flags())
    }
}

/// Shell-quotes text that would otherwise split into several arguments.
fn quote(text: &str) -> String {
    let plain = !text.is_empty()
        && text
            .chars()
            .all(|c| c.is_alphanumeric() || "_:@./-#".contains(c));
    if plain {
        text.to_string()
    } else {
        format!("'{}'", text.replace('\'', r"'\''"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn commands_omit_defaults_and_quote_text() {
        let find = Find {
            text: "open file".to_string(),
            page: Page {
                budget: 4000,
                after: 12,
            },
        };
        assert_eq!(
            find.command(),
            "cartograph find 'open file' --after 12 --budget 4000"
        );
        let explore = Explore {
            target: "@a3f9".to_string(),
            relation: Relation::Callers,
            depth: 2,
            component: Some(ComponentId(7)),
            page: Page::default(),
        };
        assert_eq!(
            explore.command(),
            "cartograph explore @a3f9 --callers --depth 2 --component 7"
        );
    }
}
