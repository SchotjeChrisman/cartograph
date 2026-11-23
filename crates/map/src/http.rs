//! The little of HTTP this server needs: request targets and replies.

use std::collections::HashMap;

use serde::Serialize;

use crate::error::Error;

pub struct Target {
    pub path: String,
    params: HashMap<String, String>,
}

impl Target {
    /// Splits `/path?key=value&...`, decoding `%XX` and `+` in the values.
    pub fn parse(url: &str) -> Self {
        let (path, query) = url.split_once('?').unwrap_or((url, ""));
        let params = query
            .split('&')
            .filter(|pair| !pair.is_empty())
            .map(|pair| {
                let (key, value) = pair.split_once('=').unwrap_or((pair, ""));
                (percent_decode(key), percent_decode(value))
            })
            .collect();
        Target {
            path: path.to_string(),
            params,
        }
    }

    pub fn param(&self, key: &str) -> Option<&str> {
        self.params.get(key).map(String::as_str)
    }
}

/// Invalid escapes are kept as written rather than rejected: the text only feeds a search or a
/// lookup that then finds nothing.
fn percent_decode(text: &str) -> String {
    let bytes = text.as_bytes();
    let mut decoded = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        let escaped = (bytes[index] == b'%')
            .then(|| bytes.get(index + 1..index + 3))
            .flatten()
            .and_then(|hex| std::str::from_utf8(hex).ok())
            .and_then(|hex| u8::from_str_radix(hex, 16).ok());
        match (bytes[index], escaped) {
            (_, Some(byte)) => {
                decoded.push(byte);
                index += 3;
            }
            (b'+', None) => {
                decoded.push(b' ');
                index += 1;
            }
            (byte, None) => {
                decoded.push(byte);
                index += 1;
            }
        }
    }
    String::from_utf8_lossy(&decoded).into_owned()
}

pub struct Reply {
    pub status: u16,
    pub content_type: String,
    pub body: Vec<u8>,
}

impl Reply {
    pub fn json<T: Serialize>(value: &T) -> Result<Self, Error> {
        // Serializing plain structs of strings and numbers cannot fail.
        let body = serde_json::to_vec(value).expect("view structs serialize to JSON");
        Ok(Reply {
            status: 200,
            content_type: "application/json".to_string(),
            body,
        })
    }

    pub fn error(error: &Error) -> Self {
        let status = match error {
            Error::BadRequest(_) => 400,
            Error::NotFound(_) => 404,
            _ => 500,
        };
        let body = serde_json::json!({ "error": error.to_string() });
        Reply {
            status,
            content_type: "application/json".to_string(),
            body: body.to_string().into_bytes(),
        }
    }

    pub fn not_found() -> Self {
        Reply::error(&Error::NotFound("no such page".to_string()))
    }

    pub fn method_not_allowed() -> Self {
        Reply {
            status: 405,
            content_type: "text/plain".to_string(),
            body: b"method not allowed".to_vec(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn query_values_are_decoded() {
        let target = Target::parse("/api/search?q=nsFoo%3A%3AInit+now&empty=&flag");

        assert_eq!(target.path, "/api/search");
        assert_eq!(target.param("q"), Some("nsFoo::Init now"));
        assert_eq!(target.param("empty"), Some(""));
        assert_eq!(target.param("flag"), Some(""));
        assert_eq!(target.param("missing"), None);
    }

    #[test]
    fn broken_escapes_survive_as_text() {
        let target = Target::parse("/x?a=100%&b=%zz&c=%e2%82%ac");

        assert_eq!(target.param("a"), Some("100%"));
        assert_eq!(target.param("b"), Some("%zz"));
        assert_eq!(target.param("c"), Some("€"));
    }
}
