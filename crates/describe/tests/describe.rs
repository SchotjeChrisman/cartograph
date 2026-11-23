use std::path::PathBuf;

use describe::{
    ComponentSlice, Describer, Error, FakeBackend, GraphSliceSource, JOB_KIND, Outcome, Overrides,
    SliceSource,
};
use graph::{
    Claim, Component, ComponentId, ContentHash, EdgeFact, EdgeKind, FileFacts, FileRecord,
    FileStatus, FileTags, GraphDir, IndexerLock, JobState, NodeFact, NodeKind, Provenance, Reader,
    SymbolId, Writer,
};

const NOW: i64 = 1_700_000_000;
const REPLY: &str = r#"{"name": "Network", "paragraph": "Talks to the wire.", "tags": ["net"]}"#;

struct Scratch(PathBuf);

impl Scratch {
    fn new(name: &str) -> Self {
        let path = PathBuf::from(env!("CARGO_TARGET_TMPDIR"))
            .join(format!("{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&path);
        std::fs::create_dir_all(&path).unwrap();
        Scratch(path)
    }

    fn dir(&self) -> GraphDir {
        GraphDir::of_root(&self.0)
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn component(id: i64, parent: Option<i64>, level: u32, name: &str, hash: &[u8]) -> Component {
    Component {
        id: ComponentId(id),
        parent: parent.map(ComponentId),
        level,
        name: name.to_string(),
        member_hash: ContentHash::of_bytes(hash),
        stale: false,
    }
}

/// System 1 with children 2 ("net") and 3 ("ui").
fn tree(net_hash: &[u8]) -> Vec<Component> {
    vec![
        component(1, None, 0, "root", b"root"),
        component(2, Some(1), 1, "net", net_hash),
        component(3, Some(1), 1, "ui", b"ui"),
    ]
}

fn store(dir: &GraphDir, components: &[Component]) -> Writer {
    let mut writer = Writer::open(dir).unwrap();
    writer.replace_components(components, &[]).unwrap();
    writer
}

struct EmptySource;

impl SliceSource for EmptySource {
    fn slice(&self, component: &Component) -> describe::Result<ComponentSlice> {
        Ok(ComponentSlice {
            name: component.name.clone(),
            level: component.level,
            file_count: 1,
            symbols: vec![],
            entry_points: vec![],
            links: vec![],
            children: vec![],
        })
    }
}

fn describer<'a>(backend: &'a FakeBackend, overrides: &'a Overrides) -> Describer<'a> {
    Describer {
        backend,
        source: &EmptySource,
        overrides,
        owner: "me".to_string(),
        token_cap: 6000,
    }
}

#[test]
fn a_description_is_generated_once_then_served_from_the_cache() {
    let scratch = Scratch::new("cache");
    let dir = scratch.dir();
    let components = tree(b"v1");
    let mut writer = store(&dir, &components);
    let reader = Reader::open(&dir).unwrap();
    let backend = FakeBackend::replying(REPLY);
    let overrides = Overrides::default();
    let describer = describer(&backend, &overrides);

    let first = describer
        .describe(&reader, &mut writer, &components[1], NOW)
        .unwrap();
    let second = describer
        .describe(&reader, &mut writer, &components[1], NOW + 1)
        .unwrap();

    assert_eq!((first, second), (Outcome::Generated, Outcome::Cached));
    assert_eq!(backend.prompts().len(), 1);
    let stored = reader.description(ComponentId(2)).unwrap().unwrap();
    assert_eq!(
        (stored.name.as_str(), stored.summary.as_str()),
        ("Network", "Talks to the wire.")
    );
    assert_eq!(stored.component_hash, components[1].member_hash);
}

#[test]
fn a_changed_hash_invalidates_the_cache_and_a_return_to_an_old_hash_regenerates() {
    let scratch = Scratch::new("invalidate");
    let dir = scratch.dir();
    let mut writer = store(&dir, &tree(b"v1"));
    let reader = Reader::open(&dir).unwrap();
    let backend = FakeBackend::replying(REPLY);
    let overrides = Overrides::default();
    let describer = describer(&backend, &overrides);

    for (step, version) in [b"v1", b"v2", b"v1"].into_iter().enumerate() {
        let components = tree(version);
        writer.replace_components(&components, &[]).unwrap();
        let outcome = describer
            .describe(&reader, &mut writer, &components[1], NOW + step as i64)
            .unwrap();
        assert_eq!(outcome, Outcome::Generated, "step {step}");
    }

    assert_eq!(backend.prompts().len(), 3);
}

#[test]
fn an_invalid_reply_is_an_error_and_leaves_no_description_and_a_failed_job() {
    let scratch = Scratch::new("invalid");
    let dir = scratch.dir();
    let components = tree(b"v1");
    let mut writer = store(&dir, &components);
    let reader = Reader::open(&dir).unwrap();
    let bad = FakeBackend::replying("sorry, no JSON today");
    let overrides = Overrides::default();

    let error = describer(&bad, &overrides)
        .describe(&reader, &mut writer, &components[1], NOW)
        .unwrap_err();

    assert!(matches!(error, Error::Output(_)), "{error}");
    assert!(reader.description(ComponentId(2)).unwrap().is_none());
    let key = format!("2:{}:none", components[1].member_hash);
    assert_eq!(
        reader.job(JOB_KIND, &key).unwrap().unwrap().state,
        JobState::Failed
    );

    let good = FakeBackend::replying(REPLY);
    let retry = describer(&good, &overrides)
        .describe(&reader, &mut writer, &components[1], NOW + 1)
        .unwrap();
    assert_eq!(
        retry,
        Outcome::Generated,
        "a failed job can be claimed again"
    );
}

#[test]
fn a_backend_failure_surfaces_and_records_the_reason() {
    let scratch = Scratch::new("backend-fail");
    let dir = scratch.dir();
    let components = tree(b"v1");
    let mut writer = store(&dir, &components);
    let reader = Reader::open(&dir).unwrap();
    let backend = FakeBackend::failing("quota exceeded");
    let overrides = Overrides::default();

    let error = describer(&backend, &overrides)
        .describe(&reader, &mut writer, &components[1], NOW)
        .unwrap_err();

    assert!(error.to_string().contains("quota exceeded"), "{error}");
    let key = format!("2:{}:none", components[1].member_hash);
    let job = reader.job(JOB_KIND, &key).unwrap().unwrap();
    assert!(job.detail.unwrap().contains("quota exceeded"));
}

#[test]
fn a_complete_override_is_stored_without_calling_the_model() {
    let scratch = Scratch::new("override-full");
    let dir = scratch.dir();
    let components = tree(b"v1");
    let mut writer = store(&dir, &components);
    let reader = Reader::open(&dir).unwrap();
    let backend = FakeBackend::replying(REPLY);
    let overrides =
        Overrides::parse("[\"net\"]\nname = \"Wire\"\nsummary = \"Mine.\"\ntags = [\"x\"]\n")
            .unwrap();

    let outcome = describer(&backend, &overrides)
        .describe(&reader, &mut writer, &components[1], NOW)
        .unwrap();

    assert_eq!(outcome, Outcome::Overridden);
    assert!(backend.prompts().is_empty());
    let stored = reader.description(ComponentId(2)).unwrap().unwrap();
    assert_eq!(
        (
            stored.name.as_str(),
            stored.summary.as_str(),
            stored.tags.as_slice()
        ),
        ("Wire", "Mine.", ["x".to_string()].as_slice())
    );
}

#[test]
fn a_partial_override_wins_over_the_model_and_survives_regeneration() {
    let scratch = Scratch::new("override-partial");
    let dir = scratch.dir();
    let mut writer = store(&dir, &tree(b"v1"));
    let reader = Reader::open(&dir).unwrap();
    let backend = FakeBackend::replying(REPLY);
    let overrides = Overrides::parse("[\"net\"]\nname = \"Wire\"\n").unwrap();
    let describer = describer(&backend, &overrides);

    for version in [b"v1", b"v2"] {
        let components = tree(version);
        writer.replace_components(&components, &[]).unwrap();
        describer
            .describe(&reader, &mut writer, &components[1], NOW)
            .unwrap();
        let stored = reader.description(ComponentId(2)).unwrap().unwrap();
        assert_eq!(stored.name, "Wire");
        assert_eq!(stored.summary, "Talks to the wire.");
    }
}

#[test]
fn a_component_claimed_by_another_process_is_not_described_twice() {
    let scratch = Scratch::new("claimed");
    let dir = scratch.dir();
    let components = tree(b"v1");
    let mut writer = store(&dir, &components);
    let reader = Reader::open(&dir).unwrap();
    let key = format!("2:{}:none", components[1].member_hash);
    let claim = writer
        .claim_job(JOB_KIND, &key, "other-agent", 900, NOW)
        .unwrap();
    assert_eq!(claim, Claim::Acquired);
    let backend = FakeBackend::replying(REPLY);
    let overrides = Overrides::default();

    let outcome = describer(&backend, &overrides)
        .describe(&reader, &mut writer, &components[1], NOW + 1)
        .unwrap();

    assert_eq!(
        outcome,
        Outcome::InProgress {
            owner: "other-agent".to_string()
        }
    );
    assert!(backend.prompts().is_empty());
}

#[test]
fn a_finished_job_without_a_description_is_reported_not_hidden() {
    let scratch = Scratch::new("done-missing");
    let dir = scratch.dir();
    let components = tree(b"v1");
    let mut writer = store(&dir, &components);
    let reader = Reader::open(&dir).unwrap();
    let key = format!("2:{}:none", components[1].member_hash);
    writer.claim_job(JOB_KIND, &key, "other", 900, NOW).unwrap();
    writer
        .finish_job(JOB_KIND, &key, "other", &graph::JobOutcome::Done, NOW)
        .unwrap();
    let backend = FakeBackend::replying(REPLY);
    let overrides = Overrides::default();

    let error = describer(&backend, &overrides)
        .describe(&reader, &mut writer, &components[1], NOW + 1)
        .unwrap_err();

    assert!(matches!(
        error,
        Error::DoneWithoutDescription { component: 2 }
    ));
}

#[test]
fn describe_down_goes_top_first_and_stops_at_the_requested_level() {
    let scratch = Scratch::new("down");
    let dir = scratch.dir();
    let mut writer = store(&dir, &tree(b"v1"));
    let reader = Reader::open(&dir).unwrap();
    let backend = FakeBackend::replying(REPLY);
    let overrides = Overrides::default();
    let describer = describer(&backend, &overrides);

    let top_only = describer
        .describe_down(&reader, &mut writer, None, 1, NOW)
        .unwrap();
    assert_eq!(top_only, vec![(ComponentId(1), Outcome::Generated)]);

    let two_levels = describer
        .describe_down(&reader, &mut writer, None, 2, NOW)
        .unwrap();
    assert_eq!(
        two_levels,
        vec![
            (ComponentId(1), Outcome::Cached),
            (ComponentId(2), Outcome::Generated),
            (ComponentId(3), Outcome::Generated),
        ]
    );
    assert_eq!(backend.prompts().len(), 3);
}

#[test]
fn a_parents_prompt_carries_its_childrens_existing_descriptions() {
    let scratch = Scratch::new("children");
    let dir = scratch.dir();
    let components = tree(b"v1");
    let mut writer = store(&dir, &components);
    let reader = Reader::open(&dir).unwrap();
    let backend = FakeBackend::replying(REPLY);
    let overrides = Overrides::default();
    let describer = describer(&backend, &overrides);
    describer
        .describe(&reader, &mut writer, &components[1], NOW)
        .unwrap();

    describer
        .describe(&reader, &mut writer, &components[0], NOW)
        .unwrap();

    let prompts = backend.prompts();
    assert!(
        prompts[1].contains("- Network: Talks to the wire."),
        "{}",
        prompts[1]
    );
    assert!(!prompts[0].contains("Parts already described"));
}

fn function_id(path: &str, name: &str) -> SymbolId {
    SymbolId::derive(NodeKind::Function, name, path)
}

fn file_with_function(path: &str, calls: Option<&str>) -> FileFacts {
    let call_edges = calls
        .map(|other| EdgeFact {
            src: function_id(path, "run"),
            dst: function_id(other, "run"),
            kind: EdgeKind::Calls,
            provenance: Provenance::Extracted,
            line: Some(2),
            reason: None,
        })
        .into_iter()
        .collect();
    FileFacts {
        file: FileRecord {
            path: path.to_string(),
            repo: String::new(),
            language: Some("rust".to_string()),
            content_hash: Some(ContentHash::of_bytes(path.as_bytes())),
            size_bytes: 10,
            mtime_ns: None,
            status: FileStatus::Parsed,
            status_detail: None,
            tags: FileTags::default(),
        },
        nodes: vec![NodeFact {
            kind: NodeKind::Function,
            name: "run".to_string(),
            qualified_name: "run".to_string(),
            line: Some(1),
            end_line: Some(3),
            signature: Some("fn run()".to_string()),
            doc: Some("Runs the part.".to_string()),
        }],
        edges: call_edges,
    }
}

#[test]
fn the_graph_source_reports_links_entry_points_and_symbols() {
    let scratch = Scratch::new("source");
    let dir = scratch.dir();
    let lock = IndexerLock::acquire(&dir).unwrap();
    let mut writer = Writer::open(&dir).unwrap();
    // ui/a.rs calls net/a.rs; net/a.rs calls nothing.
    writer
        .load(
            &lock,
            vec![
                file_with_function("net/a.rs", None),
                file_with_function("ui/a.rs", Some("net/a.rs")),
            ],
            10,
            NOW,
        )
        .unwrap();
    let components = tree(b"v1");
    writer
        .replace_components(
            &components,
            &[
                ("net/a.rs".to_string(), ComponentId(2)),
                ("ui/a.rs".to_string(), ComponentId(3)),
            ],
        )
        .unwrap();
    let reader = Reader::open(&dir).unwrap();
    let source = GraphSliceSource::load(&reader).unwrap();

    let net = source.slice(&components[1]).unwrap();
    let root = source.slice(&components[0]).unwrap();

    assert_eq!(net.file_count, 1);
    assert_eq!(net.symbols.len(), 1);
    assert_eq!(net.symbols[0].qualified_name, "run");
    assert_eq!(net.symbols[0].file.as_deref(), Some("net/a.rs"));
    assert_eq!(net.symbols[0].signature.as_deref(), Some("fn run()"));
    assert_eq!(net.symbols[0].doc.as_deref(), Some("Runs the part."));
    assert_eq!(net.entry_points.len(), 1, "ui calls into net");
    assert_eq!(net.links.len(), 1);
    assert_eq!(
        (
            net.links[0].name.as_str(),
            net.links[0].outgoing,
            net.links[0].incoming
        ),
        ("ui", 0, 1)
    );
    assert_eq!(root.file_count, 2);
    assert!(
        root.entry_points.is_empty(),
        "nothing outside the root calls in"
    );
    assert!(root.links.is_empty());
}
