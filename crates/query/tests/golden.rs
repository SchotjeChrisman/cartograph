mod common;

use common::*;
use graph::{ComponentId, IndexerLock, Reader};
use query::{
    Coverage, Engine, Explore, Find, Impact, Output, Overview, Page, PathRequest, Relation, Report,
    Request, Staleness, render,
};

fn page(budget: usize) -> Page {
    Page { budget, after: 0 }
}

fn explore(target: &str, relation: Relation, depth: u32) -> Request {
    Request::Explore(Explore {
        target: target.to_string(),
        relation,
        depth,
        component: None,
        page: Page::default(),
    })
}

fn find(text: &str) -> Request {
    Request::Find(Find {
        text: text.to_string(),
        page: Page::default(),
    })
}

fn path(from: &str, to: &str, budget: usize) -> Request {
    Request::Path(PathRequest {
        from: from.to_string(),
        to: to.to_string(),
        max_depth: 8,
        budget,
    })
}

fn impact(target: &str, depth: u32) -> Request {
    Request::Impact(Impact {
        target: target.to_string(),
        depth,
        page: Page::default(),
    })
}

struct Fixture {
    _dir: TestDir,
    reader: Reader,
}

fn fixture(name: &str) -> Fixture {
    let dir = TestDir::new(name);
    let reader = load(&dir.graph_dir());
    Fixture { _dir: dir, reader }
}

fn answer(fixture: &Fixture, request: &Request) -> Output {
    Engine::new(&fixture.reader, NOW).run(request).unwrap()
}

fn golden(name: &str, request: &Request) {
    let fixture = fixture(name);
    assert_golden(name, &render::text(&answer(&fixture, request)));
}

#[test]
fn overview_lists_top_components() {
    golden(
        "overview_top",
        &Request::Overview(Overview {
            component: None,
            page: Page::default(),
        }),
    );
}

#[test]
fn overview_drills_into_a_component_with_key_symbols() {
    golden(
        "overview_component",
        &Request::Overview(Overview {
            component: Some(ComponentId(1)),
            page: Page::default(),
        }),
    );
    golden(
        "overview_leaf",
        &Request::Overview(Overview {
            component: Some(ComponentId(2)),
            page: Page::default(),
        }),
    );
}

#[test]
fn overview_of_a_missing_component_says_so() {
    golden(
        "overview_missing",
        &Request::Overview(Overview {
            component: Some(ComponentId(99)),
            page: Page::default(),
        }),
    );
}

#[test]
fn find_ranks_exact_names_first_and_vendored_last() {
    golden("find_open", &find("open"));
}

#[test]
fn find_matches_words_inside_identifiers() {
    golden("find_fuzzy", &find("read all"));
}

#[test]
fn find_with_no_match_says_so() {
    golden("find_nothing", &find("zzzz"));
}

#[test]
fn explore_callers_two_levels_deep() {
    golden(
        "explore_callers",
        &explore("net::open", Relation::Callers, 2),
    );
}

#[test]
fn explore_callees_marks_inferred_and_unresolved() {
    golden(
        "explore_callees",
        &explore("net::Socket::connect", Relation::Callees, 1),
    );
}

#[test]
fn explore_contains_lists_a_files_definitions() {
    golden(
        "explore_contains",
        &explore("src/net/socket.rs", Relation::Contains, 1),
    );
}

#[test]
fn explore_everything_around_a_node() {
    golden(
        "explore_all",
        &explore("net::Socket::connect", Relation::All, 1),
    );
}

#[test]
fn explore_filters_by_component() {
    let request = Request::Explore(Explore {
        target: "net::open".to_string(),
        relation: Relation::Callers,
        depth: 1,
        component: Some(ComponentId(3)),
        page: Page::default(),
    });
    golden("explore_component", &request);
}

#[test]
fn ambiguous_target_lists_candidates_instead_of_guessing() {
    golden("explore_ambiguous", &explore("open", Relation::Callers, 1));
}

#[test]
fn file_and_line_resolves_to_the_enclosing_definition() {
    golden(
        "explore_location",
        &explore("src/net/socket.rs:32", Relation::Callees, 1),
    );
}

#[test]
fn path_follows_dependencies() {
    golden("path_found", &path("app::main", "zlib::inflate", 2000));
}

#[test]
fn path_elides_the_middle_when_the_budget_is_small() {
    golden("path_elided", &path("app::main", "zlib::inflate", 90));
}

#[test]
fn path_reports_when_nothing_connects() {
    golden("path_none", &path("zlib::inflate", "app::main", 2000));
}

#[test]
fn impact_walks_dependents() {
    golden("impact_open", &impact("net::read_all", 5));
}

#[test]
fn impact_respects_depth() {
    golden("impact_shallow", &impact("net::read_all", 1));
}

#[test]
fn coverage_reports_unparsed_files_and_resolution() {
    golden(
        "coverage",
        &Request::Coverage(Coverage {
            page: Page::default(),
        }),
    );
}

#[test]
fn short_ids_from_one_answer_resolve_in_the_next() {
    let fixture = fixture("short-ids");
    let Report::Find(found) = answer(&fixture, &find("connect")).report else {
        panic!("expected candidates");
    };
    let id = &found.candidates[0].short_id;
    let next = explore(&format!("@{id}"), Relation::Callees, 1);
    let Report::Explore(explored) = answer(&fixture, &next).report else {
        panic!("expected an exploration");
    };
    assert_eq!(explored.focus.node.qualified_name, "net::Socket::connect");
    assert!(id.len() >= 4);
}

#[test]
fn a_small_budget_cuts_the_list_and_the_footer_continues_it() {
    let fixture = fixture("budget");
    let base = Explore {
        target: "net::open".to_string(),
        relation: Relation::All,
        depth: 1,
        component: None,
        page: page(0),
    };
    let first = answer(&fixture, &Request::Explore(base.clone()));
    let Report::Explore(first_report) = &first.report else {
        panic!("expected an exploration");
    };
    assert_eq!(first_report.related.len(), 1, "the first item always fits");
    let more = first_report.more.as_ref().expect("a footer");
    assert_eq!(more.remaining, first_report.total - 1);
    assert!(more.command.contains("--after 1") && more.command.contains("--budget 0"));
    assert_golden("budget_cut", &render::text(&first));

    let mut seen = vec![first_report.related[0].node.short_id.clone()];
    let mut next = base;
    for _ in 0..first_report.total {
        next.page.after = seen.len();
        let Report::Explore(report) = answer(&fixture, &Request::Explore(next.clone())).report
        else {
            panic!("expected an exploration");
        };
        seen.extend(
            report
                .related
                .iter()
                .map(|related| related.node.short_id.clone()),
        );
        if report.more.is_none() {
            break;
        }
    }
    assert_eq!(seen.len(), first_report.total);
    seen.sort();
    seen.dedup();
    assert_eq!(
        seen.len(),
        first_report.total,
        "no item repeats across pages"
    );
}

#[test]
fn text_answers_stay_within_the_budget() {
    let fixture = fixture("within-budget");
    for budget in [120, 200, 400, 2000] {
        let request = Request::Explore(Explore {
            target: "net::open".to_string(),
            relation: Relation::All,
            depth: 3,
            component: None,
            page: page(budget),
        });
        let text = render::text(&answer(&fixture, &request));
        let first_item_only = text.lines().count() <= 3;
        assert!(
            query::estimate_tokens(&text) <= budget || first_item_only,
            "budget {budget} exceeded by {}",
            query::estimate_tokens(&text)
        );
    }
}

#[test]
fn json_carries_the_same_structure() {
    let fixture = fixture("json");
    let output = answer(
        &fixture,
        &explore("net::Socket::connect", Relation::Callees, 1),
    );
    let json = serde_json::to_value(&output).unwrap();
    assert_eq!(json["report"]["tool"], "explore");
    assert_eq!(
        json["report"]["focus"]["qualified_name"],
        "net::Socket::connect"
    );
    assert_eq!(json["report"]["focus"]["component_name"], "net");
    let related = json["report"]["related"].as_array().unwrap();
    assert_eq!(related.len(), 3);
    assert_eq!(related[0]["edge"]["kind"], "calls");
}

#[test]
fn indexing_in_progress_is_reported_while_another_process_holds_the_lock() {
    let dir = TestDir::new("indexing");
    let reader = load(&dir.graph_dir());
    let held = IndexerLock::acquire(&dir.graph_dir()).unwrap();
    let output = Engine::new(&reader, NOW).run(&find("open")).unwrap();
    drop(held);
    assert_eq!(output.notes.len(), 1);
    assert!(output.notes[0].contains("indexing in progress"));
    let Report::Find(found) = output.report else {
        panic!("a snapshot is still served");
    };
    assert!(!found.candidates.is_empty());
}

#[test]
fn the_staleness_hook_runs_before_each_query_and_its_findings_are_printed() {
    let fixture = fixture("staleness");
    let stale = || Ok::<_, std::io::Error>(Staleness::Stale { changed_files: 250 });
    let output = Engine::new(&fixture.reader, NOW)
        .with_staleness_check(stale)
        .run(&find("open"))
        .unwrap();
    assert_golden("stale_warning", &render::text(&output));

    let reindexed = || Ok::<_, std::io::Error>(Staleness::Reindexed { files: 3 });
    let output = Engine::new(&fixture.reader, NOW)
        .with_staleness_check(reindexed)
        .run(&find("open"))
        .unwrap();
    assert_eq!(
        output.notes,
        ["note: re-indexed 3 changed files before answering"]
    );
}

#[test]
fn a_failing_staleness_hook_fails_the_query() {
    let fixture = fixture("staleness-fails");
    let failing = || Err::<Staleness, _>(std::io::Error::other("disk gone"));
    let error = Engine::new(&fixture.reader, NOW)
        .with_staleness_check(failing)
        .run(&find("open"))
        .unwrap_err();
    assert!(error.to_string().contains("disk gone"));
}
