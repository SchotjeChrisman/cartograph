//! Detecting that stored components no longer match the files, without reclustering.

use std::collections::{HashMap, HashSet};

use graph::{ComponentId, FileRow};

use crate::identity::Previous;
use crate::tree::member_hash;
use crate::{Config, is_clustered};

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Staleness {
    /// Components whose current member hash differs from the stored one.
    pub stale: HashSet<ComponentId>,
    /// Files that should be in a component but are in none, such as files added since.
    pub unassigned_files: usize,
}

/// Compares each stored component's hash with the hash of the files now assigned to it.
pub fn check_staleness(files: &[FileRow], previous: &Previous, config: &Config) -> Staleness {
    let chains = previous.chains();
    let mut members: HashMap<ComponentId, Vec<&FileRow>> = HashMap::new();
    let mut unassigned_files = 0;
    for file in files.iter().filter(|f| is_clustered(f, config)) {
        match chains.get(file.path.as_str()) {
            Some(chain) => {
                for id in chain {
                    members.entry(*id).or_default().push(file);
                }
            }
            None => unassigned_files += 1,
        }
    }
    let stale = previous
        .components
        .iter()
        .filter(|component| {
            let current = member_hash(
                members
                    .remove(&component.id)
                    .unwrap_or_default()
                    .into_iter(),
            );
            current != component.member_hash
        })
        .map(|component| component.id)
        .collect();
    Staleness {
        stale,
        unassigned_files,
    }
}
