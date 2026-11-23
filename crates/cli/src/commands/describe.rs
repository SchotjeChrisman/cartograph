use std::path::Path;

use describe::{
    DEFAULT_TOKEN_CAP, Describer, GraphSliceSource, Outcome, Overrides, ProcessBackend,
};
use graph::{ComponentId, GraphDir, Reader, Writer};
use serde_json::json;

use crate::Result;
use crate::args::DescribeArgs;

pub fn run(root: &Path, args: &DescribeArgs, as_json: bool) -> Result<String> {
    let dir = GraphDir::of_root(root);
    let reader = Reader::open(&dir)?;
    let mut writer = Writer::open(&dir)?;
    let backend = ProcessBackend::from_name(&args.backend)?;
    let source = GraphSliceSource::load(&reader)?;
    let overrides = Overrides::load(&dir)?;
    let describer = Describer {
        backend: &backend,
        source: &source,
        overrides: &overrides,
        owner: format!("cli-{}", std::process::id()),
        token_cap: DEFAULT_TOKEN_CAP,
    };
    let outcomes = describer.describe_down(
        &reader,
        &mut writer,
        args.component.map(ComponentId),
        args.levels(),
        graph::now(),
    )?;
    let lines = outcomes
        .iter()
        .map(|(id, outcome)| (id.0, outcome_label(outcome)));
    Ok(if as_json {
        let rows: Vec<_> = lines
            .map(|(id, outcome)| json!({ "component": id, "outcome": outcome }))
            .collect();
        serde_json::to_string(&rows)?
    } else {
        lines
            .map(|(id, outcome)| format!("component {id}: {outcome}"))
            .collect::<Vec<_>>()
            .join("\n")
    })
}

fn outcome_label(outcome: &Outcome) -> String {
    match outcome {
        Outcome::Cached => "already described".to_string(),
        Outcome::Generated => "described".to_string(),
        Outcome::Overridden => "written from overrides".to_string(),
        Outcome::InProgress { owner } => format!("being described by {owner}"),
    }
}
