// The sidebar flavour: the component tree of the focused folder, with the selected component's
// details under it.
import { div } from "gpui-kit";
import { v_flex, h_flex } from "gpui-base";
import { agents, focused, panes, run, spaces, theme } from "herdr";
import { executable } from "./cli.js";
import { GraphSource } from "./source.js";
import { chooseAgent, firstSentence, folderName, footerText } from "./model.js";
import { button, componentDetail, notesBanner, staleMark, stateMessage } from "./parts.js";

const MOD_NAME = "cartograph";
const INDENT = 14;
const DETAIL_HEIGHT = 300;

// Where the person is working: the focused pane's live folder, else the shown space's.
function location() {
  const space = spaces().find((candidate) => candidate.active);
  const focus = focused();
  const pane = focus?.pane ? panes().find((candidate) => candidate.pane === focus.pane) : null;
  // A focused terminal outside the tab (a peek, a layout block) can live on another machine,
  // which only its address says (`machine/t7`), as in the shipped files mod.
  const isOutsideTab = focus?.pane === null && focus?.terminal?.includes("/");
  const on = isOutsideTab ? focus.terminal.split("/")[0] : space?.machine ?? null;
  return { folder: pane?.cwd || focus?.cwd || space?.folder || null, on, space: space?.id ?? null };
}

export class Sidebar {
  init(props) {
    this.executable = executable(props);
    /** @type {Map<string, { source: GraphSource, expanded: Set<number>, selected: any }>} */
    this.places = new Map();
  }

  place(where) {
    const key = JSON.stringify([where.on, where.folder]);
    if (!this.places.has(key)) {
      const source = new GraphSource({ folder: where.folder, on: where.on, executable: this.executable });
      this.places.set(key, { source, expanded: new Set(), selected: null });
    }
    return this.places.get(key);
  }

  ensureLevel(source, componentId, cx) {
    if (source.level(componentId) !== undefined) return;
    cx.spawn(async (cx) => {
      await source.loadLevel(componentId);
      cx.notify();
    });
  }

  ensureInfo(source, cx) {
    if (source.info) return;
    cx.spawn(async (cx) => {
      await source.loadInfo();
      cx.notify();
    });
  }

  refresh(place, cx) {
    place.source.clear();
    cx.notify();
  }

  toggle(place, node, cx) {
    if (place.expanded.has(node.id)) place.expanded.delete(node.id);
    else place.expanded.add(node.id);
    cx.notify();
  }

  select(place, node, cx) {
    place.selected = node;
    cx.notify();
  }

  openFullView(where, repo) {
    const terminal = chooseAgent(focused(), agents(), where.space);
    run("tab.new", { name: `Map · ${repo}`, mod: MOD_NAME, props: { view: "full", folder: where.folder, on: where.on, terminal, cartograph: this.executable } });
    // The new tab is focused and its only pane is the map, so this split lands beside it. Attaching
    // shows the agent's own terminal rather than a copy; closing the pane only detaches.
    if (terminal) run("pane.split", { direction: "right", argv: ["hc", "attach", terminal] });
  }

  treeRows(t, place, parentId, depth, rows, cx) {
    const level = place.source.level(parentId);
    this.ensureLevel(place.source, parentId, cx);
    if (level?.status !== "ready") {
      if (depth > 0) rows.push(div().pl(8 + depth * INDENT).py(3).text_size(11).text_color(t.dim).child(level?.status === "loading" || !level ? "Loading…" : level.detail));
      return rows;
    }
    for (const node of level.children) {
      rows.push(this.treeRow(t, place, node, depth, cx));
      if (place.expanded.has(node.id)) this.treeRows(t, place, node.id, depth + 1, rows, cx);
    }
    if (level.hiddenChildren > 0) rows.push(div().pl(8 + depth * INDENT).py(3).text_size(11).text_color(t.dim).child(`${level.hiddenChildren} more not shown`));
    return rows;
  }

  treeRow(t, place, node, depth, cx) {
    const below = place.source.level(node.id);
    const isLeaf = below?.status === "ready" && below.children.length === 0;
    const isOpen = place.expanded.has(node.id);
    const isSelected = place.selected?.id === node.id;
    const textColor = node.isStale ? t.dim : t.foreground;
    return h_flex().id(`node-${node.id}`).flex_none().pl(4 + depth * INDENT).pr(8).py(3).gap(4).items_start().rounded(6).cursor_pointer()
      .when(isSelected, (el) => el.bg(t.selected)).hover((s) => s.bg(isSelected ? t.selected : t.hover))
      .on_click((_event, cx) => this.select(place, node, cx))
      .child(div().id(`toggle-${node.id}`).w(14).flex_none().text_size(10).text_color(t.dim).pt(2)
        // On press, and stopped there, so the row's own click (select) doesn't fire as well.
        .when(!isLeaf, (el) => el.on_mouse_down("left", (_event, cx) => { cx.stop_propagation(); this.toggle(place, node, cx); }))
        .child(isLeaf ? "" : isOpen ? "▾" : "▸"))
      .child(v_flex().flex_1().min_w(0)
        .child(h_flex().gap(5).min_w(0)
          .child(div().min_w(0).truncate().text_size(13).text_color(textColor).child(node.name))
          .when(node.isStale, (el) => el.child(staleMark(t))))
        .when(Boolean(node.summary), (el) => el.child(div().truncate().text_size(11).text_color(node.isStale ? t.dim : t.muted).child(firstSentence(node.summary ?? "")))));
  }

  header(t, place, repo, folder) {
    const refresh = place ? button(t, "refresh", "Refresh", (_event, cx) => this.refresh(place, cx)) : null;
    return h_flex().h(42).flex_none().px(12).gap(8).items_center().border_b(1).border_color(t.border)
      .child(v_flex().flex_1().min_w(0)
        .child(div().truncate().font_semibold().text_size(13).child(repo || "Cartograph"))
        .child(div().truncate().text_size(10).text_color(t.dim).child(folder ?? "No folder")))
      .when(Boolean(refresh), (el) => el.child(refresh));
  }

  footer(t, place, where, repo) {
    const info = place.source.info;
    const text = info?.status === "failed" ? info.detail : footerText(info, Date.now() / 1000);
    const isReady = place.source.level(null)?.status === "ready";
    return h_flex().flex_none().px(12).py(8).gap(8).items_center().border_t(1).border_color(t.border)
      .child(div().flex_1().min_w(0).truncate().text_size(11).text_color(t.dim).child(text))
      .child(button(t, "open-full", "Open full view", () => this.openFullView(where, repo), { isPrimary: true, isDisabled: !isReady }));
  }

  render(cx) {
    const t = theme();
    const where = location();
    const shell = v_flex().id("cartograph").size_full().min_h(0).bg(t.background).text_color(t.foreground).text_size(13);
    if (!where.folder) return shell.child(this.header(t, null, "", null)).child(div().p(12).text_color(t.dim).child("Focus a space or an agent to see its map."));
    const place = this.place(where);
    this.ensureInfo(place.source, cx);
    const top = place.source.level(null);
    this.ensureLevel(place.source, null, cx);
    const repo = place.source.info?.repo ?? folderName(where.folder);
    const problem = stateMessage(t, top, where.folder);
    const selected = place.selected;
    return shell
      .child(this.header(t, place, repo, where.folder))
      .when(Boolean(notesBanner(t, top)), (el) => el.child(notesBanner(t, top)))
      .child(problem ?? v_flex().id("tree").flex_1().min_h(0).overflow_y_scrollbar().p(6).children(this.treeRows(t, place, null, 0, [], cx)))
      .when(Boolean(problem), (el) => el.child(div().flex_1()))
      .when(Boolean(selected) && !problem, (el) => {
        this.ensureLevel(place.source, selected.id, cx);
        return el.child(v_flex().id("detail").flex_none().h(DETAIL_HEIGHT).overflow_y_scrollbar().p(12).border_t(1).border_color(t.border)
          .child(componentDetail(t, selected, place.source.level(selected.id))));
      })
      .child(this.footer(t, place, where, repo));
  }
}
