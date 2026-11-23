//! Keeps component ids stable across runs: a new component takes the id of the old one that
//! shares most of its files, so descriptions written for it survive edits.

use std::collections::HashMap;

use graph::{Component, ComponentId, Reader};

/// Two components are the same one when their files overlap at least this much (Jaccard).
const MIN_SIMILARITY: f64 = 0.5;

/// The tree as stored before this run.
#[derive(Debug, Clone, Default)]
pub struct Previous {
    pub components: Vec<Component>,
    /// Each clustered file and the deepest component it was assigned to.
    pub file_components: Vec<(String, ComponentId)>,
}

impl Previous {
    pub fn load(reader: &Reader) -> graph::Result<Self> {
        let mut components = Vec::new();
        let mut pending = reader.components(None)?;
        while let Some(component) = pending.pop() {
            pending.extend(reader.components(Some(component.id))?);
            components.push(component);
        }
        components.sort_by_key(|component| component.id);
        let file_components = reader
            .files()?
            .into_iter()
            .filter_map(|file| Some((file.path, file.component?)))
            .collect();
        Ok(Previous {
            components,
            file_components,
        })
    }

    pub fn max_id(&self) -> i64 {
        self.components.iter().map(|c| c.id.0).max().unwrap_or(0)
    }

    /// For every file path, the ids of the old components that contain it, deepest first.
    pub(crate) fn chains(&self) -> HashMap<&str, Vec<ComponentId>> {
        let parent_of: HashMap<ComponentId, Option<ComponentId>> = self
            .components
            .iter()
            .map(|component| (component.id, component.parent))
            .collect();
        self.file_components
            .iter()
            .map(|(path, leaf)| {
                let mut chain = vec![*leaf];
                while let Some(Some(parent)) =
                    parent_of.get(chain.last().expect("chain starts non-empty"))
                {
                    chain.push(*parent);
                }
                (path.as_str(), chain)
            })
            .collect()
    }
}

/// What a freshly computed component needs to be matched: its level and the paths it holds.
pub struct Candidate<'a> {
    pub level: u32,
    pub paths: Vec<&'a str>,
}

/// One id per candidate: the id of the best-overlapping old component, or a new id above every
/// old one. Each old id is given out at most once, to its best claimant.
pub fn assign_ids(candidates: &[Candidate], previous: &Previous) -> Vec<ComponentId> {
    let chains = previous.chains();
    let old_level: HashMap<ComponentId, u32> = previous
        .components
        .iter()
        .map(|component| (component.id, component.level))
        .collect();
    let mut old_size: HashMap<ComponentId, usize> = HashMap::new();
    for chain in chains.values() {
        for id in chain {
            *old_size.entry(*id).or_default() += 1;
        }
    }

    let mut offers = Vec::new();
    for (index, candidate) in candidates.iter().enumerate() {
        let mut overlap: HashMap<ComponentId, usize> = HashMap::new();
        for path in &candidate.paths {
            for id in chains.get(path).into_iter().flatten() {
                *overlap.entry(*id).or_default() += 1;
            }
        }
        for (old, shared) in overlap {
            let union = candidate.paths.len() + old_size[&old] - shared;
            let similarity = shared as f64 / union as f64;
            if similarity >= MIN_SIMILARITY {
                let same_level = old_level.get(&old) == Some(&candidate.level);
                offers.push((similarity, same_level, old, index));
            }
        }
    }
    offers.sort_by(|a, b| {
        b.0.total_cmp(&a.0)
            .then(b.1.cmp(&a.1))
            .then(a.2.cmp(&b.2))
            .then(a.3.cmp(&b.3))
    });

    let mut assigned: Vec<Option<ComponentId>> = vec![None; candidates.len()];
    let mut taken = std::collections::HashSet::new();
    for (_, _, old, index) in offers {
        if assigned[index].is_none() && taken.insert(old) {
            assigned[index] = Some(old);
        }
    }
    let mut next_id = previous.max_id();
    assigned
        .into_iter()
        .map(|id| {
            id.unwrap_or_else(|| {
                next_id += 1;
                ComponentId(next_id)
            })
        })
        .collect()
}
