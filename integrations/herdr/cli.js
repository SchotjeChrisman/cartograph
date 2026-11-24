// Runs cartograph (and the one shell script that finds its graph) for a folder. The only module
// that starts processes; what the output means is model.js's job.
import { exec } from "herdr";
import { COVERAGE_ARGS, exploreArgs, overviewArgs, parseLocation, runResult, startFailure, stepOutcome } from "./model.js";

// A query re-indexes up to 100 changed files inline before answering, which can take a while.
const QUERY_TIMEOUT_MS = 120 * 1000;
const LOCATE_TIMEOUT_MS = 10 * 1000;

// Walks up like the CLI's workspace::graph_root does, then falls back to the one linked worktree
// directly inside the folder that holds a graph (a bare-clone project's top); several such
// worktrees count as none, as the CLI refuses to guess. `date -r <file>` prints a file's
// modification time on both BSD and GNU date; the WAL file is newer while writes wait for a
// checkpoint.
const LOCATE_SCRIPT = `found() {
  printf '%s\\n' "$1"
  for f in "$1/.cartograph/graph.db" "$1/.cartograph/graph.db-wal"; do [ -f "$f" ] && date -r "$f" +%s; done
  exit 0
}
d=$1
while :; do
  [ -f "$d/.cartograph/graph.db" ] && found "$d"
  [ "$d" = / ] && break
  d=$(dirname "$d")
done
only= count=0
for w in "$1"/*/ "$1"/.[!.]*/; do
  w=\${w%/}
  [ -f "$w/.cartograph/graph.db" ] && grep -Eqs '^gitdir: *(.*/)?worktrees/' "$w/.git" || continue
  only=$w count=$((count + 1))
done
[ "$count" = 1 ] && found "$only"
exit 1`;

const DEFAULT_EXECUTABLE = "cartograph";
const EXECUTABLE_KEY = "cartograph";

// Herdr gives exec the login shell's PATH, which misses folders only an interactive shell config
// adds, such as ~/.local/bin where release binaries are installed. Going through sh lets $HOME
// expand on the machine that runs it, local or remote.
const DEFAULT_LAUNCHER = `PATH="$HOME/.local/bin:$HOME/.cargo/bin:$PATH" exec ${DEFAULT_EXECUTABLE} "$@"`;

/** @param {Target} target @param {string[]} args @returns {[string, string[]]} */
function commandLine(target, args) {
  return target.executable === DEFAULT_EXECUTABLE
    ? ["sh", ["-c", DEFAULT_LAUNCHER, DEFAULT_EXECUTABLE, ...args]]
    : [target.executable, args];
}

/** @typedef {{ folder: string, on: string | null, executable: string }} Target */

// A prop (from a layout block's content or the tab that opened this pane) wins over the copy's
// stored setting, so one pane can try another build without touching the others.
export function executable(props) {
  let stored = null;
  try { stored = localStorage.getItem(EXECUTABLE_KEY); } catch (error) { console.log(`cartograph: executable setting not read: ${error}`); }
  return props.cartograph ?? stored ?? DEFAULT_EXECUTABLE;
}

/** @param {Target} target @param {string[]} args */
async function cartograph(target, args) {
  try {
    const [program, programArgs] = commandLine(target, args);
    const result = await exec(program, programArgs, { cwd: target.folder, on: target.on, timeout: QUERY_TIMEOUT_MS });
    return runResult(result, target.executable);
  } catch (error) {
    return startFailure(target.executable, error);
  }
}

/** @param {Target} target @param {number | null} componentId */
export const overview = (target, componentId) => cartograph(target, overviewArgs(componentId));

/** @param {Target} target @param {string} shortId */
export const explore = (target, shortId) => cartograph(target, exploreArgs(shortId));

/** @param {Target} target */
export const coverage = (target) => cartograph(target, COVERAGE_ARGS);

/** The graph root above the folder and when its database was last written; null without one.
 *  @param {Target} target */
export async function locateGraph(target) {
  const result = await exec("sh", ["-c", LOCATE_SCRIPT, "locate", target.folder], { on: target.on, timeout: LOCATE_TIMEOUT_MS });
  return result.code === 0 ? parseLocation(result.stdout) : null;
}

/** One step of a run command (model.js runSteps), whose output is a message for the person, not JSON.
 *  Failing to start counts as a failed step, with the reason as its text.
 *  @param {Target} target @param {{ args: string[], timeoutMs: number }} step */
export async function runStep(target, step) {
  try {
    const [program, programArgs] = commandLine(target, step.args);
    const result = await exec(program, programArgs, { cwd: target.folder, on: target.on, timeout: step.timeoutMs });
    return stepOutcome(result);
  } catch (error) {
    return { isFailure: true, text: startFailure(target.executable, error).detail };
  }
}
