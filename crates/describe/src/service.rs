//! Describing components: cache check, claim, prompt, model call, validation, storage.

use graph::{Claim, Component, ComponentId, Description, JobOutcome, Reader, UnixSeconds, Writer};

use crate::backend::Backend;
use crate::error::{Error, Result};
use crate::output::{self, Generated};
use crate::overrides::{Override, Overrides};
use crate::prompt::{ChildSummary, build_prompt};
use crate::source::SliceSource;

/// Jobs of this kind in the graph's jobs table are describe runs.
pub const JOB_KIND: &str = "describe";

/// How long a claim is honoured before another process may take the work over; generous because
/// a model call can take minutes.
pub const CLAIM_LEASE_SECONDS: i64 = 900;

/// What happened to one component.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Outcome {
    /// A description for the component's current hash already existed; no model call.
    Cached,
    /// The model wrote a new description.
    Generated,
    /// A person wrote every field, so it was stored without a model call.
    Overridden,
    /// Another process holds the claim; its result will show up in the graph.
    InProgress { owner: String },
}

pub struct Describer<'a> {
    pub backend: &'a dyn Backend,
    pub source: &'a dyn SliceSource,
    pub overrides: &'a Overrides,
    /// Names this process in job claims.
    pub owner: String,
    pub token_cap: usize,
}

impl Describer<'_> {
    /// Describes `component` unless a description for its current hash exists.
    pub fn describe(
        &self,
        reader: &Reader,
        writer: &mut Writer,
        component: &Component,
        now: UnixSeconds,
    ) -> Result<Outcome> {
        if let Some(entry) = self.overrides.for_component(&component.name)
            && entry.is_complete()
        {
            writer.put_description(&from_override(component, entry, now))?;
            return Ok(Outcome::Overridden);
        }
        let existing = reader.description(component.id)?;
        if existing
            .as_ref()
            .is_some_and(|d| !d.is_stale_for(component))
        {
            return Ok(Outcome::Cached);
        }
        let key = job_key(component, existing.as_ref());
        match writer.claim_job(JOB_KIND, &key, &self.owner, CLAIM_LEASE_SECONDS, now)? {
            Claim::HeldBy { owner } => Ok(Outcome::InProgress { owner }),
            Claim::AlreadyDone => self.after_finished_elsewhere(reader, component),
            Claim::Acquired => self.generate_and_store(reader, writer, component, &key, now),
        }
    }

    /// Describes `start` and everything below it, parents before children, `levels` levels deep
    /// (1 means just `start`). `start` of `None` means the top-level components.
    pub fn describe_down(
        &self,
        reader: &Reader,
        writer: &mut Writer,
        start: Option<ComponentId>,
        levels: u32,
        now: UnixSeconds,
    ) -> Result<Vec<(ComponentId, Outcome)>> {
        let mut frontier = match start {
            Some(id) => reader.component(id)?.into_iter().collect(),
            None => reader.components(None)?,
        };
        let mut outcomes = Vec::new();
        for _ in 0..levels {
            let mut next = Vec::new();
            for component in &frontier {
                let outcome = self.describe(reader, writer, component, now)?;
                outcomes.push((component.id, outcome));
                next.extend(reader.components(Some(component.id))?);
            }
            if next.is_empty() {
                break;
            }
            frontier = next;
        }
        Ok(outcomes)
    }

    /// Another process finished this exact work between our cache check and our claim.
    fn after_finished_elsewhere(&self, reader: &Reader, component: &Component) -> Result<Outcome> {
        match reader.description(component.id)? {
            Some(description) if !description.is_stale_for(component) => Ok(Outcome::Cached),
            _ => Err(Error::DoneWithoutDescription {
                component: component.id.0,
            }),
        }
    }

    fn generate_and_store(
        &self,
        reader: &Reader,
        writer: &mut Writer,
        component: &Component,
        key: &str,
        now: UnixSeconds,
    ) -> Result<Outcome> {
        let result = self
            .generate(reader, component)
            .map(|generated| self.to_description(component, generated, now));
        match result {
            Ok(description) => {
                writer.put_description(&description)?;
                writer.finish_job(JOB_KIND, key, &self.owner, &JobOutcome::Done, now)?;
                Ok(Outcome::Generated)
            }
            Err(error) => {
                writer.finish_job(
                    JOB_KIND,
                    key,
                    &self.owner,
                    &JobOutcome::Failed(error.to_string()),
                    now,
                )?;
                Err(error)
            }
        }
    }

    fn generate(&self, reader: &Reader, component: &Component) -> Result<Generated> {
        let mut slice = self.source.slice(component)?;
        slice.children = self.described_children(reader, component.id)?;
        let reply = self
            .backend
            .complete(&build_prompt(&slice, self.token_cap))?;
        Ok(output::parse_reply(&reply)?)
    }

    fn described_children(
        &self,
        reader: &Reader,
        parent: ComponentId,
    ) -> Result<Vec<ChildSummary>> {
        let mut summaries = Vec::new();
        for child in reader.components(Some(parent))? {
            if let Some(description) = reader.description(child.id)? {
                let description = self.overrides.apply(&child.name, description);
                summaries.push(ChildSummary {
                    name: description.name,
                    summary: description.summary,
                });
            }
        }
        Ok(summaries)
    }

    /// The model's text with any partial override laid over it, so a person's words win even
    /// in a description the model wrote.
    fn to_description(
        &self,
        component: &Component,
        generated: Generated,
        now: UnixSeconds,
    ) -> Description {
        let description = Description {
            component: component.id,
            component_hash: component.member_hash,
            name: generated.name,
            summary: generated.paragraph,
            tags: generated.tags,
            written_at: now,
        };
        self.overrides.apply(&component.name, description)
    }
}

/// Names the exact work: this component at this hash, replacing that earlier description. The
/// earlier hash is part of the key because a component can return to an old hash, and a finished
/// key is never run again.
fn job_key(component: &Component, existing: Option<&Description>) -> String {
    let replaced = existing.map_or_else(|| "none".to_string(), |d| d.component_hash.to_string());
    format!("{}:{}:{replaced}", component.id.0, component.member_hash)
}

fn from_override(component: &Component, entry: &Override, now: UnixSeconds) -> Description {
    Description {
        component: component.id,
        component_hash: component.member_hash,
        name: entry.name.clone().unwrap_or_default(),
        summary: entry.summary.clone().unwrap_or_default(),
        tags: entry.tags.clone().unwrap_or_default(),
        written_at: now,
    }
}
