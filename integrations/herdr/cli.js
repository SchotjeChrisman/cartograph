// Runs cartograph (and the one shell script that finds its graph) for a folder. The only module
// that starts processes; what the output means is model.js's job.
import { exec } from "herdr";
import { COVERAGE_ARGS, exploreArgs, overviewArgs, parseLocation, runResult, startFailure } from "./model.js";

// A query re-indexes up to 100 changed files inline before answering, which can take a while.
const QUERY_TIMEOUT_MS = 120 * 1000;
const LOCATE_TIMEOUT_MS = 10 * 1000;

// Walks up like the CLI's workspace::graph_root does. `date -r <file>` prints a file's
// modification time on both BSD and GNU date; the WAL file is newer while writes wait for a
// checkpoint.
const LOCATE_SCRIPT = `d=$1
while :; do
  if [ -f "$d/.cartograph/graph.db" ]; then
    printf '%s\\n' "$d"
    for f in "$d/.cartograph/graph.db" "$d/.cartograph/graph.db-wal"; do [ -f "$f" ] && date -r "$f" +%s; done
    exit 0
  fi
  [ "$d" = / ] && exit 1
  d=$(dirname "$d")
done`;

const DEFAULT_EXECUTABLE = "cartograph";
const EXECUTABLE_KEY = "cartograph";

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
    const result = await exec(target.executable, args, { cwd: target.folder, on: target.on, timeout: QUERY_TIMEOUT_MS });
    return runResult(result);
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
