//! How the frontend names a box: `c<component id>` or `f<file id>`.

use std::fmt;

use graph::ComponentId;

use crate::error::{Error, Result};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum NodeId {
    Component(ComponentId),
    File(i64),
}

impl NodeId {
    pub fn parse(text: &str) -> Result<Self> {
        let invalid = || Error::BadRequest(format!("`{text}` is not a node id like c12 or f7"));
        let number = text.get(1..).and_then(|digits| digits.parse::<i64>().ok());
        match (text.chars().next(), number) {
            (Some('c'), Some(id)) => Ok(NodeId::Component(ComponentId(id))),
            (Some('f'), Some(id)) => Ok(NodeId::File(id)),
            _ => Err(invalid()),
        }
    }
}

impl fmt::Display for NodeId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            NodeId::Component(id) => write!(formatter, "c{}", id.0),
            NodeId::File(id) => write!(formatter, "f{id}"),
        }
    }
}

impl serde::Serialize for NodeId {
    fn serialize<S: serde::Serializer>(
        &self,
        serializer: S,
    ) -> std::result::Result<S::Ok, S::Error> {
        serializer.collect_str(self)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ids_round_trip_through_text() {
        for id in [NodeId::Component(ComponentId(12)), NodeId::File(7)] {
            assert_eq!(NodeId::parse(&id.to_string()).unwrap(), id);
        }
    }

    #[test]
    fn malformed_ids_are_bad_requests() {
        for text in ["", "c", "x1", "c1x", "12", "é1"] {
            assert!(
                matches!(NodeId::parse(text), Err(Error::BadRequest(_))),
                "{text}"
            );
        }
    }
}
