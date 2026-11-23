// One folder's graph as the views see it: each level, link list and footer fetched once and kept
// until refresh. Views read synchronously in render and ask for loads from cx.spawn.
import * as cli from "./cli.js";
import { fileCount, LINK_SYMBOL_COUNT, overviewLevel, strongestLinks } from "./model.js";

const LOADING = { status: "loading" };
const TOP_LEVEL = "top";

const levelKey = (componentId) => componentId ?? TOP_LEVEL;

export class GraphSource {
  /** @param {import("./cli.js").Target} target */
  constructor(target) {
    this.target = target;
    this.clear();
  }

  clear() {
    this.generation = {};
    this.levels = new Map();
    this.loads = new Map();
    this.links = new Map();
    this.info = null;
  }

  /** The level below a component (null: the top), or undefined before it is asked for. */
  level(componentId) {
    return this.levels.get(levelKey(componentId));
  }

  linksOf(componentId) {
    return this.links.get(componentId);
  }

  /** Resolves when the level is in; a load already under way is shared, not repeated. */
  loadLevel(componentId) {
    const key = levelKey(componentId);
    if (!this.loads.has(key)) {
      const levels = this.levels;
      levels.set(key, LOADING);
      this.loads.set(key, cli.overview(this.target, componentId).then((result) => {
        levels.set(key, overviewLevel(result));
      }));
    }
    return this.loads.get(key);
  }

  async loadInfo() {
    if (this.info) return;
    const generation = this.generation;
    this.info = LOADING;
    const info = await this.readInfo();
    if (generation === this.generation) this.info = info;
  }

  async readInfo() {
    try {
      const [location, coverage] = await Promise.all([cli.locateGraph(this.target), cli.coverage(this.target)]);
      return { status: "ready", repo: location?.repo ?? null, writtenAt: location?.writtenAt ?? null, fileCount: fileCount(coverage) };
    } catch (error) {
      return { status: "failed", detail: String(error instanceof Error ? error.message : error) };
    }
  }

  /** Needs the component's own level loaded first: its key symbols are where the links start. */
  async loadLinks(componentId) {
    const links = this.links;
    const level = this.level(componentId);
    if (links.has(componentId) || level?.status !== "ready") return;
    links.set(componentId, LOADING);
    const symbols = level.keySymbols.slice(0, LINK_SYMBOL_COUNT);
    const results = [];
    for (const symbol of symbols) results.push(await cli.explore(this.target, symbol.shortId));
    links.set(componentId, strongestLinks(results, await this.loadDescendants(componentId)));
  }

  // Symbols name their leaf component, so a link counts as internal only when every component
  // below this one is known; the levels loaded here also serve later expands and drill-downs.
  async loadDescendants(componentId) {
    const ids = new Set([componentId]);
    const pending = [componentId];
    while (pending.length) {
      const parentId = pending.pop();
      await this.loadLevel(parentId);
      const level = this.level(parentId);
      if (level?.status !== "ready") continue;
      for (const child of level.children) {
        ids.add(child.id);
        pending.push(child.id);
      }
    }
    return ids;
  }
}
