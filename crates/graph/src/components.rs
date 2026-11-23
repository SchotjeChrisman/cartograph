//! Components and their descriptions: written by cluster and describe, read by everyone.

use rusqlite::{OptionalExtension, Row, TransactionBehavior, params};
use serde::Serialize;

use crate::error::{Error, Result};
use crate::id::ContentHash;
use crate::store::{Reader, UnixSeconds, Writer};

const TAG_SEPARATOR: char = '\n';

/// Identity of a component. Cluster keeps it stable across runs by member overlap.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize)]
#[serde(transparent)]
pub struct ComponentId(pub i64);

/// One node of the system > subsystem > component tree; files are the leaves.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Component {
    pub id: ComponentId,
    pub parent: Option<ComponentId>,
    /// Depth in the tree, 0 for a top-level system.
    pub level: u32,
    pub name: String,
    /// Hash of the component's members; a description is current only for the hash it was written for.
    pub member_hash: ContentHash,
    /// Set when a member file changed since the component was computed.
    pub stale: bool,
}

/// The model-written (or overridden) text for a component.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Description {
    pub component: ComponentId,
    /// The member hash the text was written for.
    pub component_hash: ContentHash,
    pub name: String,
    pub summary: String,
    pub tags: Vec<String>,
    pub written_at: UnixSeconds,
}

impl Description {
    /// Whether the component's code has moved on since this description was written.
    pub fn is_stale_for(&self, component: &Component) -> bool {
        self.component_hash != component.member_hash
    }
}

impl Writer {
    /// Replaces the whole component tree and the file assignments. Descriptions of components
    /// that no longer exist are dropped; those of surviving ids are kept.
    pub fn replace_components(
        &mut self,
        components: &[Component],
        file_components: &[(String, ComponentId)],
    ) -> Result<()> {
        let transaction = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        transaction.execute("DELETE FROM components", [])?;
        transaction.execute("UPDATE files SET component_id = NULL", [])?;
        for component in components {
            transaction.execute(
                "INSERT INTO components (id, parent_id, level, name, member_hash, stale)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                params![
                    component.id.0,
                    component.parent.map(|parent| parent.0),
                    component.level,
                    component.name,
                    component.member_hash.as_bytes().as_slice(),
                    component.stale
                ],
            )?;
        }
        for (path, component) in file_components {
            transaction.execute(
                "UPDATE files SET component_id = ?1 WHERE path = ?2",
                params![component.0, path],
            )?;
        }
        transaction.execute(
            "DELETE FROM descriptions WHERE component_id NOT IN (SELECT id FROM components)",
            [],
        )?;
        Ok(transaction.commit()?)
    }

    /// Stores or replaces a component's description.
    pub fn put_description(&mut self, description: &Description) -> Result<()> {
        if description
            .tags
            .iter()
            .any(|tag| tag.contains(TAG_SEPARATOR))
        {
            return Err(Error::InvalidFact("a tag contains a newline".to_string()));
        }
        self.connection.execute(
            "INSERT INTO descriptions (component_id, component_hash, name, summary, tags, written_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)
             ON CONFLICT (component_id) DO UPDATE SET component_hash = excluded.component_hash,
             name = excluded.name, summary = excluded.summary, tags = excluded.tags,
             written_at = excluded.written_at",
            params![
                description.component.0,
                description.component_hash.as_bytes().as_slice(),
                description.name,
                description.summary,
                description.tags.join(&TAG_SEPARATOR.to_string()),
                description.written_at
            ],
        )?;
        Ok(())
    }
}

impl Reader {
    /// The children of `parent`, or the top-level components for `None`, ordered by name.
    pub fn components(&self, parent: Option<ComponentId>) -> Result<Vec<Component>> {
        let mut statement = self.connection.prepare_cached(
            "SELECT id, parent_id, level, name, member_hash, stale FROM components
             WHERE parent_id IS ?1 ORDER BY name",
        )?;
        let rows = statement.query_map([parent.map(|parent| parent.0)], component_from_row)?;
        rows.map(|row| row?).collect()
    }

    pub fn component(&self, id: ComponentId) -> Result<Option<Component>> {
        let row = self
            .connection
            .query_row(
                "SELECT id, parent_id, level, name, member_hash, stale FROM components WHERE id = ?1",
                [id.0],
                component_from_row,
            )
            .optional()?;
        row.transpose()
    }

    pub fn description(&self, component: ComponentId) -> Result<Option<Description>> {
        let row = self
            .connection
            .query_row(
                "SELECT component_hash, name, summary, tags, written_at FROM descriptions
                 WHERE component_id = ?1",
                [component.0],
                |row| {
                    let tags: String = row.get(3)?;
                    Ok((
                        row.get::<_, Vec<u8>>(0)?,
                        row.get(1)?,
                        row.get(2)?,
                        tags,
                        row.get(4)?,
                    ))
                },
            )
            .optional()?;
        row.map(|(hash, name, summary, tags, written_at)| {
            Ok(Description {
                component,
                component_hash: decode_hash(&hash)?,
                name,
                summary,
                tags: tags
                    .split(TAG_SEPARATOR)
                    .filter(|tag| !tag.is_empty())
                    .map(String::from)
                    .collect(),
                written_at,
            })
        })
        .transpose()
    }
}

fn component_from_row(row: &Row) -> rusqlite::Result<Result<Component>> {
    let id: i64 = row.get(0)?;
    let parent: Option<i64> = row.get(1)?;
    let level: u32 = row.get(2)?;
    let name: String = row.get(3)?;
    let hash: Vec<u8> = row.get(4)?;
    let stale: bool = row.get(5)?;
    Ok(decode_hash(&hash).map(|member_hash| Component {
        id: ComponentId(id),
        parent: parent.map(ComponentId),
        level,
        name,
        member_hash,
        stale,
    }))
}

fn decode_hash(bytes: &[u8]) -> Result<ContentHash> {
    ContentHash::from_slice(bytes)
        .ok_or_else(|| Error::CorruptValue(format!("hash of {} bytes", bytes.len())))
}
