// Run with: node --test integrations/herdr/model.test.mjs
import { test } from "node:test";
import assert from "node:assert/strict";
import {
  age, askPrompt, chooseAgent, componentNode, fileCount, firstSentence, footerText, overviewArgs,
  overviewCommand, overviewLevel, parseLocation, runFolder, runRefreshesTree, runResult, runSteps, startFailure, stepOutcome, strongestLinks,
} from "./model.js";

const componentView = ({ id, stale = false, description = null, descriptionStale = false }) => ({
  component: { id, parent: null, level: 0, name: `generated-${id}`, member_hash: "ab", stale },
  description,
  description_stale: descriptionStale,
});
const description = { component: 1, component_hash: "ab", name: "Engine", summary: "Indexes code. Then answers queries.", tags: ["core"], written_at: 1 };
const ready = (report, notes = []) => ({ status: "ready", output: { notes, report } });
const nodeRef = (component, name) => ({ short_id: "a1", id: "a100000000000000", kind: "function", name, qualified_name: name, file: "a.rs", line: 3, component, degree: 1, tags: { vendored: false, generated: false }, component_name: name });

test("a described component shows its description's name and stale follows either flag", () => {
  assert.deepEqual(componentNode(componentView({ id: 1, description })), { id: 1, level: 0, name: "Engine", summary: description.summary, tags: ["core"], isStale: false });
  assert.equal(componentNode(componentView({ id: 2 })).name, "generated-2");
  assert.equal(componentNode(componentView({ id: 3, stale: true })).isStale, true);
  assert.equal(componentNode(componentView({ id: 4, description, descriptionStale: true })).isStale, true);
});

test("runs are classified: JSON, no graph, other failures, and a missing executable", () => {
  assert.equal(runResult({ code: 0, stdout: '{"notes":[],"report":{}}', stderr: "" }).status, "ready");
  assert.equal(runResult({ code: 1, stdout: "", stderr: "error: no graph at /x/.cartograph/graph.db; run `cartograph index` first" }).status, "no-graph");
  assert.deepEqual(runResult({ code: 1, stdout: "", stderr: "error: boom\n" }), { status: "failed", detail: "error: boom" });
  assert.match(runResult({ code: 0, stdout: "not json", stderr: "" }).detail, /not JSON/);
  assert.deepEqual(runResult({ code: 127, stdout: "", stderr: "sh: cartograph: not found\n" }, "cartograph"),
    { status: "not-installed", executable: "cartograph", detail: "sh: cartograph: not found" });
  assert.equal(startFailure("cartograph", new Error("cannot start cartograph: program not found")).status, "not-installed");
  assert.equal(startFailure("cartograph", new Error("timed out")).status, "failed");
});

test("an overview becomes a level with notes read", () => {
  const level = overviewLevel(ready({ tool: "overview", parent: componentView({ id: 1, description }), components: [componentView({ id: 2 })], key_symbols: [nodeRef(1, "run")], more: { remaining: 4, exact: true, command: "x" } }, ["note: indexing in progress in another process; showing the last committed snapshot"]));
  assert.equal(level.parent.name, "Engine");
  assert.deepEqual(level.children.map((child) => child.id), [2]);
  assert.deepEqual(level.keySymbols, [{ shortId: "a1", kind: "function", name: "run", location: "a.rs:3" }]);
  assert.equal(level.hiddenChildren, 4);
  assert.equal(level.isIndexing, true);
  assert.equal(level.isEmpty, false);
  assert.equal(overviewLevel(ready({ tool: "not_found", query: "component 9" })).status, "failed");
  assert.equal(overviewLevel({ status: "no-graph", detail: "d" }).status, "no-graph");
});

test("arguments and commands match the CLI", () => {
  assert.deepEqual(overviewArgs(null), ["--json", "overview", "--budget", "50000"]);
  assert.deepEqual(overviewArgs(7), ["--json", "overview", "--component", "7", "--budget", "50000"]);
  assert.equal(overviewCommand(null), "cartograph overview");
  assert.equal(overviewCommand(7), "cartograph overview --component 7");
});

test("summaries cut at the first sentence like the text renderer", () => {
  assert.equal(firstSentence("Indexes code. Then answers."), "Indexes code.");
  assert.equal(firstSentence("No end"), "No end");
});

test("footer: file count, location and age", () => {
  assert.equal(fileCount(ready({ tool: "coverage", inputs: { files: [{ count: 2 }, { count: 3 }] } })), 5);
  assert.equal(fileCount({ status: "failed" }), null);
  assert.deepEqual(parseLocation("/a/repo\n100\n250\n"), { root: "/a/repo", repo: "repo", writtenAt: 250 });
  assert.equal(parseLocation(""), null);
  assert.deepEqual([age(5), age(120), age(7200), age(200000)], ["5s", "2m", "2h", "2d"]);
  assert.equal(footerText({ fileCount: 1234, writtenAt: 0 }, 3600), "1,234 files · graph written 1h ago");
  assert.equal(footerText(null, 0), "");
});

test("links count neighbours outside the box, drop structural and unowned ones, strongest first", () => {
  const related = (component, direction, kind = "calls") => ({ node: nodeRef(component, `c${component}`), depth: 1, direction, edge: { kind }, via: null });
  const explore = ready({ tool: "explore", related: [related(5, "out"), related(5, "in"), related(6, "out"), related(2, "out"), related(7, "in", "contains"), related(null, "out")] });
  const links = strongestLinks([explore, { status: "failed" }], new Set([1, 2]));
  assert.deepEqual(links.map((link) => [link.component, link.outgoing, link.incoming]), [[5, 1, 1], [6, 1, 0]]);
});

test("the agent is the focused one, else the shown space's first", () => {
  const list = [{ address: "t1", space: "a", focused: false }, { address: "t2", space: "b", focused: false }];
  assert.equal(chooseAgent({ agent: "t2" }, list, "a"), "t2");
  assert.equal(chooseAgent({ terminal: "t9" }, list, "b"), "t2");
  assert.equal(chooseAgent(null, list, "c"), null);
});

test("the prompt names the component and the command, on one line", () => {
  const prompt = askPrompt({ id: 4, name: "Query" });
  assert.match(prompt, /"Query" component \(#4\)/);
  assert.match(prompt, /cartograph overview --component 4/);
  assert.equal(prompt.includes("\n"), false);
});

test("update graph indexes then clusters; describe uses the CLI defaults unless one component is named", () => {
  assert.deepEqual(runSteps("update").map((step) => [step.label, step.args]), [["Indexing…", ["index"]], ["Clustering…", ["cluster"]]]);
  assert.deepEqual(runSteps("describe").map((step) => step.args), [["describe"]]);
  assert.deepEqual(runSteps("describeComponent", 7).map((step) => step.args), [["describe", "--component", "7"]]);
  assert.deepEqual(runSteps("install").map((step) => step.args), [["install"]]);
  assert.equal(runSteps("describe")[0].timeoutMs, 600000);
  assert.deepEqual([runRefreshesTree("update"), runRefreshesTree("describeComponent"), runRefreshesTree("install")], [true, true, false]);
});

test("a run starts in the graph root, else the focused folder", () => {
  assert.equal(runFolder("/a", "/a/src"), "/a");
  assert.equal(runFolder(null, "/b"), "/b");
});

test("a step shows its last output line, or all of stderr when it fails", () => {
  assert.deepEqual(stepOutcome({ code: 0, stdout: "scanning\nindexed 12 files\n", stderr: "" }), { isFailure: false, text: "indexed 12 files" });
  assert.deepEqual(stepOutcome({ code: 0, stdout: "", stderr: "" }), { isFailure: false, text: "Done." });
  assert.deepEqual(stepOutcome({ code: 1, stdout: "partial", stderr: "error: a\nerror: b\n" }), { isFailure: true, text: "error: a\nerror: b" });
  assert.equal(stepOutcome({ code: 2, stdout: "", stderr: "" }).text, "cartograph exited with code 2");
});
