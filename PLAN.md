# Cartograph — plan

Agreed with the user in chat, 2026-11-20. The user does not read this file; anything they need to see goes in chat. Design only: nothing is built yet. The decision-by-decision log is DESIGN-STATE.md.

## Goal
A local tool that maps a very large codebase into a queryable graph, then draws a plain-English interactive map from that same graph. It combines what Graphify (queryable graph for agents) and Floorplan (readable map for humans) each do half of. Research behind it: Graphify (Graphify-Labs/graphify, Apache-2.0, tree-sitter, about 40 languages, offline for code) and Floorplan (haydenschmitty/floorplan, MIT, the model reads the code to describe it, always needs a cloud model call).

## Scale and concurrency
- Built from the start for the biggest projects. Yardstick: Gecko (mozilla-central), tens of millions of LOC and several hundred thousand files (size from memory, verify when the corpus is set up). Gecko is not on the dev machine yet; the budgets below were measured on a depth-1 Linux kernel clone (97,267 files, 68,699 indexed, 1.88M nodes and 6.81M edges loaded) by `scripts/bench.sh`, on a 6-core host that other work kept at a load average of 20 to 25, so wall times are upper bounds. Re-measure on Gecko when it is set up; memory and time grow linearly with files.
- Budgets (Linux kernel, 68.7k indexed files; measured then rounded up):
  - Full index: 5 minutes wall under that load (283 s measured, 365 CPU-seconds; the pre-optimisation indexer took 483 s and 466 CPU-seconds in the same conditions). Peak memory 1.5 GiB (1469 MiB measured; it was 2.7 GiB). On disk: `graph.db` 1.2 GiB plus the indexer's `extraction-cache.db` 1.8 GiB.
  - Incremental index: 1 edited file 10 s (8.3 s measured), 100 edited files 12 s (10.2 s), nothing changed 2 s (1.8 s; was 20 to 27 s for every run). Peak memory 1 GiB (749 and 928 MiB, 142 MiB when nothing changed).
  - Query latency: 1 s at p50 for find, callers, impact and overview (755 to 856 ms measured). Almost all of it is the per-query staleness check (a walk of the tree plus a read of the file table), which grows with files and is still the next thing to cut.
  - Concurrency: 30 simultaneous queries on a stale tree start exactly one indexer, the other 29 report the stale warning and "indexing in progress", no errors.
  - Cluster: 15 s and 143 MiB for 65k files (388 components). Map top level (`/api/level`): 11 s at p50, over budget: it recomputes the cross-file links of all 6.8M edges on every request; storing or caching them is open work.
- Dozens of agents and any number of map views query one graph at the same time.

## Decisions
- Own tree-sitter indexer; Graphify is only a test oracle for cross-checking edge counts and missed files.
- Rust. One static binary. Nix flake plus GitHub release binaries (static Linux x86_64 and aarch64, macOS).
- One-shot CLI. `cartograph mcp` is a thin stdio process over the same graph file. No daemon; a foreground `cartograph watch` comes later.
- One graph per directory root, in `.cartograph/` inside it; the developer chooses monorepo root or per-repo. Everything below the root is one graph; files carry a repo tag from the nearest .git; a nested `.cartograph/` is skipped and reported.
- Describer is pluggable: `claude -p`, `codex`, `ollama` (fully offline).

## Layers (Cargo workspace by feature; dependencies point one way: cli -> query/describe/map/cluster/index -> graph)
Crates: graph (types + SQLite store), index (walker + parsing), languages (one module per language and framework plugin), cluster, query, describe (backends), map (server + embedded frontend), cli (binary + MCP).

1. **index**: walks the root (respects .gitignore and `.cartographignore`), parses with tree-sitter in parallel, emits nodes and edges. Incremental by content hash, with a size plus modification-time check that skips reading a file the graph already hashed (only trusted for files left alone for 2 seconds before the run). What each file extracted lives in an `extraction-cache.db` beside the graph, so a run holds only the small part other files can see (definitions and non-call references) and builds facts one file at a time. Pure functions from file bytes to facts; I/O only in the walker.
2. **store**: SQLite in WAL mode. Integer ids, edge table indexed both directions, FTS5 for names, batched writes from one writer thread while parsers run in parallel. A first load goes in as one transaction with the secondary indexes and the word index built at the end, which beat row-by-row index upkeep because symbol ids are random 63-bit hashes. Deep traversals (impact, path) run in Rust over a small in-memory CSR slice built per query.
3. **cluster**: Leiden at several resolutions gives a tree: system > subsystem > component > file. Directory structure seeds and biases it so names match developer expectations. Ids match across runs by member overlap so descriptions survive edits.
4. **query**: see Query contract.
5. **describe**: lazy and top-down. `cartograph describe` fills the top levels; deeper components when requested (CLI, `overview`, or opened in the map), cached by component hash. Input per call: the component's members (top symbols with signatures and doc comments), strongest edges to other components, entry points, children's descriptions; capped at about 6k tokens; no file bodies. Output: name, one paragraph, tags, validated as JSON. Overrides in `.cartograph/overrides.toml` always win. Index and map work with zero model calls.
6. **map**: see Map.

## Graph model
- Node kinds: file, module, symbol (function, method, class, type), route, table, config key, hook.
- Edge kinds: contains, imports, calls, inherits, implements, reads, writes, handles_route, registers_hook, fires_hook, declared.
- Every edge: provenance (`extracted`, `inferred`, `declared`), file and line, and for `inferred` a short reason. Unresolved calls are kept as edges to an `unresolved` node so coverage is measurable.
- Symbol ids are stable across re-indexing: a hash of qualified name + file path + kind.

## Query contract
- Output is agents first: dense plain text, one line per node or edge with file:line; `--json` gives the full structure; MCP returns the same structure.
- Every query takes `--budget` (default about 2k tokens). Results are ranked (nearest, most connected, same component first), cut at the budget, and end with a footer: how many more exist and the exact follow-up command (`--component`, `--depth`, `--after <cursor>`).
- Name lookup is fuzzy (FTS5). Ambiguous names return ranked candidates (qualified name, file:line, component, edge count) each with a short stable id usable next (`callers @a3f9`). Qualified names and `file:line` resolve directly.
- Six MCP tools, each with `budget`: `overview`, `find`, `explore` (callers, callees, contains, depth), `path`, `impact`, `coverage`. The CLI mirrors them with aliases such as `callers` and `callees`.

## Languages and frameworks
- First: C/C++, Rust, JavaScript/TypeScript, PHP with Laravel as the first framework plugin (target: Mainstay). Python is not in the first set; its files show up as unparsed in `coverage`.
- Each language is a tree-sitter grammar, a `.scm` queries file and an optional small Rust module for resolution rules; frameworks have the same shape layered on a language. All compiled in, extra grammars behind Cargo features. No WASM plugins.
- C/C++: heuristics by default (include search over the tree, calls by name and enclosing scope, tagged `inferred`, ambiguous calls left unresolved); `compile_commands.json`, when present, gives exact include paths, defines and overload sets. No libclang.
- Each language has a fixture corpus of tricky cases that must resolve before it counts as supported.

## Freshness and concurrency
- Files keyed by content hash. `index` re-parses only changed files (git diff plus mtime check for dirty trees). Every query runs a cheap staleness check: few changed files re-index inline, many give a one-line warning with the graph's age. Affected clusters and descriptions are marked stale, not recomputed inline.
- Many processes share one SQLite file. Readers never block on the writer. A lock file makes one process the indexer; the others serve the last committed snapshot and report "indexing in progress". A `jobs` table with claim rows deduplicates index and describe work, which also stops 30 agents all re-indexing the same stale files.
- Every query sits behind a small trait so a shared server can be added later if a benchmark (30 concurrent agents on Gecko) shows the file approach is not enough.

## Map
- `cartograph map` is a foreground command running a local HTTP server (bind 0.0.0.0, chosen port, offline, no external assets, frontend embedded in the binary). It streams only the visible level from SQLite and can trigger `describe` for an opened component. The server lives only while the command runs. No static export.
- Boxes and arrows, one level at a time: the children of one node as named boxes with the description beneath, arrows for the strongest dependencies (thickness = edge count), at most about 50 boxes on screen. Click drills in, breadcrumb goes up, a side panel shows description, key symbols and the equivalent `cartograph` command.
- Frontend: React, with UI SDK (@ui-sdk/core, @ui-sdk/react, Vercel, Apache-2.0, beta) for panels and behaviours (side panel, breadcrumb, search combobox, trees, dialogs), and a canvas library (PixiJS or Konva, chosen at build time) drawing the boxes and arrows with our own layered layout. UI SDK stays limited to panels because it is beta and DOM-based. Each is a dependency needing approval at build time.
- No chat panel in the map. Search and drill-down only.

## Agent integration
- `cartograph install` registers the MCP server and adds a small skill/CLAUDE.md snippet saying when to call `overview`, `find`, `explore` first. Works for Claude Code, Codex and any MCP client.
- Optional opt-in strict hook (PreToolUse, like Graphify's `--strict`) denies Read and Grep until a graph query has oriented the agent in the session. Off by default; it must allow Read and Grep when `coverage` says the relevant files are unparsed.

## Skip rules and privacy
- The walk respects .gitignore and `.cartographignore` (same syntax, read only by cartograph; the generic `.ignore` is not read, so ripgrep and cartograph exclusions stay separate). Vendored (third_party/, vendor/, node_modules/, *.min.js) and generated (generated-file headers) paths are indexed but tagged, de-ranked in queries and excluded from components by default (`--include-vendored`). Patterns are overridable in `.cartograph/config.toml`.
- Secret files (.env, *.pem, key files, credential patterns) are never parsed or stored; `coverage` lists what was skipped and why.
- No telemetry, no network except the chosen describer backend. Query logs stay local.

## Differences from the two reference tools
- Cheaper map: the model reads per-component graph slices, so cost scales with the number of components, not files.
- Map and agent queries read the same graph.
- Parse failures are loud: coverage per language and per file. (Graphify silently drops files when a parser fails.)
- Blind spots declared: queues, cron, webhooks and runtime routes go in `links.yaml` in the target repo and become `declared` edges.
- Staleness shown: each description records the component hash it was written for; the map greys out components whose code has moved on.
- Fully offline possible with the ollama backend.

## Milestones
1. **Index + store**, C/C++ and Rust first (Gecko), then TypeScript and PHP. `index`, `coverage`, `callers`. Cross-check against Graphify on two real repos; differences explained or fixed. First Gecko measurement sets the numeric budgets.
2. **Query layer + MCP + concurrency.** Six tools, budgets and footers, fuzzy lookup with stable ids, WAL multi-process with the indexer lock and jobs table, incremental re-index; a 30-agent test on Gecko.
3. **Cluster + describe.** Hierarchical components, pluggable backends, hash-keyed cache, overrides.
4. **Map.** Served React map, canvas boxes and arrows, panels, lazy describe from the map; headless-browser check.
5. **Laravel plugin, `links.yaml`, agent install and strict hook.**
6. After real use: `watch`, more languages and git-diff impact summaries.

## Tests
- Per language: fixture repo with expected edges as a golden file; the test fails if an expected edge is missing or an unexpected `unresolved` appears.
- Incremental index equals a clean full index.
- Describe: fake backend; cache hit and invalidation by hash; JSON validation.
- Concurrency: many readers during a write; one indexer under 30 simultaneous stale queries.
- Map: headless render of a fixture graph, no console errors.
- Benchmark script against Gecko recording index time, memory and query latency.

## Herdr mod
- A Herdr desktop app mod (Herdr.app 0.1.16; the API is `~/.config/herdr-app/AGENTS.md`, `herdr.d.ts` and `gpui-kit.d.ts`). Source in `integrations/herdr/`, installed by symlinking `~/.config/herdr-app/mods/cartograph` to it. Plain JavaScript on Herdr's gpui-kit, no dependencies.
- Data comes only from the CLI: `cartograph --json overview [--component <id>]` per expand, run with `exec` in the folder, cached per folder and component until Refresh. No Rust changes, no server.
- Sidebar (default, docked on the left next to the agents): follows the focused agent's or space's folder; the system > subsystem > component tree, foldable, name and one-line description per node, stale components greyed with "(stale)"; the selected node's description, key symbols and command under the tree; repo name and Refresh on top, file count and graph age at the bottom; clear states for cartograph not on PATH, no graph (shows `cartograph index`), indexing in progress and CLI errors (stderr shown as is); "Open full view".
- Full view (the same mod with props `{view: "full", folder, terminal}`): a new tab with the map and, split beside it, the focused agent's own terminal (`hc attach`), so the chat is the agent itself. Breadcrumb, the current component's children as boxes with descriptions, click drills in; a detail panel with description, key symbols, the equivalent command and "Ask agent about this", which types a prompt naming the component and its command into the agent's terminal (`hc send`).
- No arrows between boxes: the CLI JSON has no component-to-component links (they exist only in the map server's `/api/level`). The detail panel lists the strongest links instead, counted from `explore` on the box's key symbols.
- A `cartograph` prop, or the mod copy's `cartograph` localStorage key, points the mod at another executable (used for testing with a fake).

## Still to settle at build time
- Describer backend trait signature and overrides.toml format.
- Canvas library (PixiJS or Konva).
- Dependency list (tree-sitter crates, rusqlite, a Leiden implementation, React, UI SDK, canvas library): each needs Chris's approval before it is added.
- Embedding-based search: leaning FTS only.
