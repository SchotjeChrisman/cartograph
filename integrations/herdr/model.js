// What the mod shows, worked out from cartograph's `--json` output. Pure: no Herdr, no exec, no
// clock, so it runs under plain node for the tests. The JSON shapes are query::Output and the
// report structs in crates/query/src/report.rs.

// Large enough that the overview's token budget never cuts a level short in a panel; the
// key symbols are only sent when every child fitted (crates/query/src/overview.rs).
export const OVERVIEW_BUDGET = 50000;
const EXPLORE_BUDGET = 8000;
// Each key symbol costs one `explore` call when a box's links are worked out.
export const LINK_SYMBOL_COUNT = 5;
const LINK_COUNT = 8;
// The same cut as render::first_sentence in crates/query/src/render.rs.
const SENTENCE_END = ". ";
const EMPTY_GRAPH_NOTE = "the graph is empty";
const INDEXING_NOTE = "indexing in progress";
const NO_GRAPH_ERROR = /no graph at (.+?); run `cartograph index`/;
const NOT_INSTALLED_ERROR = /program not found|No such file or directory|ENOENT/i;
// The exit code sh uses when the program it was told to exec does not exist.
const SHELL_COMMAND_NOT_FOUND = 127;
const MINUTE = 60;
const HOUR = 60 * MINUTE;
const DAY = 24 * HOUR;

export function overviewArgs(componentId) {
  const component = componentId == null ? [] : ["--component", String(componentId)];
  return ["--json", "overview", ...component, "--budget", String(OVERVIEW_BUDGET)];
}

export function exploreArgs(shortId) {
  return ["--json", "explore", `@${shortId}`, "--budget", String(EXPLORE_BUDGET)];
}

export const COVERAGE_ARGS = ["--json", "coverage"];

/** What an agent would type for the same view, as query::Overview::command() prints it. */
export function overviewCommand(componentId) {
  return componentId == null ? "cartograph overview" : `cartograph overview --component ${componentId}`;
}

/** Why the executable could not be started: missing, or something else worth showing as is. */
export function startFailure(executable, error) {
  const detail = String(error instanceof Error ? error.message : error);
  return NOT_INSTALLED_ERROR.test(detail)
    ? { status: "not-installed", executable, detail }
    : { status: "failed", detail };
}

/** One finished `cartograph --json ...` run: its parsed query::Output, or why there is none. */
export function runResult(execResult, executable) {
  if (execResult.code === SHELL_COMMAND_NOT_FOUND) {
    return { status: "not-installed", executable, detail: execResult.stderr.trim() };
  }
  if (execResult.code !== 0) {
    const stderr = execResult.stderr.trim() || `cartograph exited with code ${execResult.code}`;
    const noGraph = stderr.match(NO_GRAPH_ERROR);
    return noGraph ? { status: "no-graph", detail: stderr } : { status: "failed", detail: stderr };
  }
  try {
    return { status: "ready", output: JSON.parse(execResult.stdout) };
  } catch (error) {
    return { status: "failed", detail: `cartograph printed something that is not JSON (${error.message}):\n${execResult.stdout.slice(0, 500)}` };
  }
}

/** The text before the first sentence end, so a summary fits one line. */
export function firstSentence(summary) {
  const end = summary.indexOf(SENTENCE_END);
  return end === -1 ? summary : summary.slice(0, end + 1);
}

/** A query::ComponentView as a tree node; stale as the served map judges it (map/src/labels.rs). */
export function componentNode(view) {
  const { component, description } = view;
  return {
    id: component.id,
    level: component.level,
    name: description?.name ?? component.name,
    summary: description?.summary ?? null,
    tags: description?.tags ?? [],
    isStale: component.stale || view.description_stale,
  };
}

/** A query::NodeRef (graph::NodeInfo flattened into it) as a line in a symbol list. */
export function symbolEntry(nodeRef) {
  return {
    shortId: nodeRef.short_id,
    kind: nodeRef.kind,
    name: nodeRef.qualified_name,
    location: nodeRef.file ? (nodeRef.line ? `${nodeRef.file}:${nodeRef.line}` : nodeRef.file) : null,
  };
}

/** The notes above every answer (crates/query/src/fresh.rs), as flags plus the text to show. */
export function readNotes(notes) {
  return {
    isIndexing: notes.some((note) => note.includes(INDEXING_NOTE)),
    isEmpty: notes.some((note) => note.includes(EMPTY_GRAPH_NOTE)),
    notes,
  };
}

/** An `overview` run as one level of the tree: the parent, its children and its key symbols. */
export function overviewLevel(result) {
  if (result.status !== "ready") return result;
  const { notes, report } = result.output;
  if (report.tool === "not_found") return { status: "failed", detail: `cartograph found no ${report.query}` };
  if (report.tool !== "overview") return { status: "failed", detail: `expected an overview, got ${report.tool}` };
  return {
    status: "ready",
    ...readNotes(notes),
    parent: report.parent ? componentNode(report.parent) : null,
    children: report.components.map(componentNode),
    keySymbols: report.key_symbols.map(symbolEntry),
    hiddenChildren: report.more?.remaining ?? 0,
  };
}

/** Files in the graph: the sum of coverage's per-language counts (graph::CoverageInputs.files). */
export function fileCount(result) {
  if (result.status !== "ready" || result.output.report.tool !== "coverage") return null;
  return result.output.report.inputs.files.reduce((sum, entry) => sum + entry.count, 0);
}

const FIVE_MINUTES_MS = 5 * MINUTE * 1000;
// The longest a single exec may run (Herdr's cap); describe calls a model per component.
const EXEC_MAX_MS = 10 * MINUTE * 1000;

/** The buttons that run cartograph, by id: their steps and whether the tree must be reloaded after. */
const RUN_COMMANDS = {
  update: { refreshesTree: true, steps: () => [
    { label: "Indexing…", args: ["index"], timeoutMs: FIVE_MINUTES_MS },
    { label: "Clustering…", args: ["cluster"], timeoutMs: FIVE_MINUTES_MS },
  ] },
  describe: { refreshesTree: true, steps: () => [{ label: "Describing…", args: ["describe"], timeoutMs: EXEC_MAX_MS }] },
  describeComponent: { refreshesTree: true, steps: (componentId) => [{ label: "Describing…", args: ["describe", "--component", String(componentId)], timeoutMs: EXEC_MAX_MS }] },
  install: { refreshesTree: false, steps: () => [{ label: "Installing…", args: ["install"], timeoutMs: FIVE_MINUTES_MS }] },
};

/** The ordered steps of a run command; `componentId` only matters to describeComponent. */
export const runSteps = (commandId, componentId = null) => RUN_COMMANDS[commandId].steps(componentId);

export const runRefreshesTree = (commandId) => RUN_COMMANDS[commandId].refreshesTree;

/** Where a run starts: the graph root when there is one, else the focused folder (a new graph). */
export const runFolder = (graphRoot, focusedFolder) => graphRoot ?? focusedFolder;

const lastLine = (text) => text.trim().split("\n").pop();

/** One finished step: the last meaningful output line, or on failure everything it said on stderr. */
export function stepOutcome(execResult) {
  if (execResult.code === 0) {
    return { isFailure: false, text: lastLine(execResult.stdout) || lastLine(execResult.stderr) || "Done." };
  }
  const detail = execResult.stderr.trim() || execResult.stdout.trim();
  return { isFailure: true, text: detail || `cartograph exited with code ${execResult.code}` };
}

export const folderName = (folder) => folder?.split("/").filter(Boolean).pop() ?? "";

/** The locate script's output: the graph root on the first line, then file times in seconds. */
export function parseLocation(stdout) {
  const [root, ...times] = stdout.trim().split("\n");
  if (!root) return null;
  const writtenAt = times.map(Number).filter(Number.isFinite);
  return { root, repo: folderName(root) || root, writtenAt: writtenAt.length ? Math.max(...writtenAt) : null };
}

/** How long ago, in the same units as the stale-graph warning (fresh.rs `age`). */
export function age(seconds) {
  const s = Math.max(0, Math.floor(seconds));
  if (s >= DAY) return `${Math.floor(s / DAY)}d`;
  if (s >= HOUR) return `${Math.floor(s / HOUR)}h`;
  if (s >= MINUTE) return `${Math.floor(s / MINUTE)}m`;
  return `${s}s`;
}

/** The footer line: how big the graph is and when it was last written. */
export function footerText(info, nowSeconds) {
  const parts = [];
  if (info?.fileCount != null) parts.push(`${info.fileCount.toLocaleString("en")} files`);
  if (info?.writtenAt != null) parts.push(`graph written ${age(nowSeconds - info.writtenAt)} ago`);
  return parts.join(" · ");
}

/**
 * The components a box talks to most, from `explore` runs on its key symbols: the CLI has no
 * component-to-component links (those exist only in the served map's /api/level), so this
 * counts the direct neighbours of the box's best-connected symbols. `ownIds` are the box and
 * the descendants known so far; links into them are internal.
 */
export function strongestLinks(exploreResults, ownIds) {
  const byComponent = new Map();
  for (const result of exploreResults) {
    if (result.status !== "ready" || result.output.report.tool !== "explore") continue;
    for (const related of result.output.report.related) {
      const { component, component_name: name } = related.node;
      if (component == null || ownIds.has(component) || related.edge.kind === "contains") continue;
      const link = byComponent.get(component) ?? { component, name: name ?? `#${component}`, outgoing: 0, incoming: 0 };
      if (related.direction === "out") link.outgoing += 1;
      else link.incoming += 1;
      byComponent.set(component, link);
    }
  }
  return [...byComponent.values()]
    .sort((a, b) => b.outgoing + b.incoming - (a.outgoing + a.incoming) || a.name.localeCompare(b.name))
    .slice(0, LINK_COUNT);
}

/** The agent the full view talks to: the focused one, else the first agent of the shown space. */
export function chooseAgent(focus, agentList, spaceId) {
  const focusedAddress = focus?.agent ?? focus?.terminal;
  const focusedAgent = agentList.find((agent) => agent.address === focusedAddress || agent.focused);
  return (focusedAgent ?? agentList.find((agent) => agent.space === spaceId))?.address ?? null;
}

/** What "Ask agent about this" types into the agent's terminal: one line, since send presses enter. */
export function askPrompt(node) {
  return `Using cartograph, explain the "${node.name}" component (#${node.id}): what it does, its key symbols and what it depends on. Start with \`${overviewCommand(node.id)}\` and explore from there.`;
}
