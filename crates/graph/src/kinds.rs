//! Node kinds, edge kinds and provenance. Codes are stored in SQLite and must never be renumbered.

use std::fmt;

use serde::Serialize;

/// Declares a fieldless enum with stable numeric codes and snake_case names.
macro_rules! coded_enum {
    ($(#[$meta:meta])* $name:ident { $($variant:ident = $code:literal => $text:literal),+ $(,)? }) => {
        $(#[$meta])*
        #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize)]
        #[serde(rename_all = "snake_case")]
        pub enum $name {
            $($variant = $code),+
        }

        impl $name {
            /// Every variant, in code order.
            pub const ALL: &'static [$name] = &[$($name::$variant),+];

            /// The code stored in the database.
            pub fn code(self) -> i64 {
                self as i64
            }

            /// Inverse of [`Self::code`]; `None` for an unknown code.
            pub fn from_code(code: i64) -> Option<Self> {
                match code {
                    $($code => Some($name::$variant),)+
                    _ => None,
                }
            }

            /// The snake_case name used in text output and on the command line.
            pub fn as_str(self) -> &'static str {
                match self {
                    $($name::$variant => $text),+
                }
            }

            /// Inverse of [`Self::as_str`].
            pub fn parse(text: &str) -> Option<Self> {
                match text {
                    $($text => Some($name::$variant),)+
                    _ => None,
                }
            }
        }

        impl fmt::Display for $name {
            fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
                formatter.write_str(self.as_str())
            }
        }
    };
}

coded_enum! {
    /// What a graph node is.
    NodeKind {
        File = 0 => "file",
        Module = 1 => "module",
        Function = 2 => "function",
        Method = 3 => "method",
        Class = 4 => "class",
        Type = 5 => "type",
        Route = 6 => "route",
        Table = 7 => "table",
        ConfigKey = 8 => "config_key",
        Hook = 9 => "hook",
        Unresolved = 10 => "unresolved",
    }
}

impl NodeKind {
    /// Global nodes are shared by every file that mentions them, so their identity ignores the
    /// file path and they belong to no file.
    pub fn is_global(self) -> bool {
        matches!(
            self,
            NodeKind::Unresolved | NodeKind::Table | NodeKind::ConfigKey | NodeKind::Hook
        )
    }
}

coded_enum! {
    /// What an edge says about its two nodes.
    EdgeKind {
        Contains = 0 => "contains",
        Imports = 1 => "imports",
        Calls = 2 => "calls",
        Inherits = 3 => "inherits",
        Implements = 4 => "implements",
        Reads = 5 => "reads",
        Writes = 6 => "writes",
        HandlesRoute = 7 => "handles_route",
        RegistersHook = 8 => "registers_hook",
        FiresHook = 9 => "fires_hook",
        Declared = 10 => "declared",
    }
}

coded_enum! {
    /// How an edge became known.
    Provenance {
        Extracted = 0 => "extracted",
        Inferred = 1 => "inferred",
        Declared = 2 => "declared",
    }
}

coded_enum! {
    /// What indexing did with a file; everything but `Parsed` shows up in coverage.
    FileStatus {
        Parsed = 0 => "parsed",
        Unparsed = 1 => "unparsed",
        ParseFailed = 2 => "parse_failed",
        SkippedSecret = 3 => "skipped_secret",
        SkippedNestedGraph = 4 => "skipped_nested_graph",
    }
}

coded_enum! {
    /// Where a job stands.
    JobState {
        Claimed = 0 => "claimed",
        Done = 1 => "done",
        Failed = 2 => "failed",
    }
}
