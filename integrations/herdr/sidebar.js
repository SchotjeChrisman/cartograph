// The sidebar flavour: the component tree of the focused folder, with the selected component's
// details under it.
import { div } from "gpui-kit";
import { v_flex, h_flex } from "gpui-base";
import { agents, focused, panes, run, spaces, theme } from "herdr";
import { executable } from "./cli.js";
import { GraphSource } from "./source.js";
import { Runs } from "./runs.js";
import { chooseAgent, firstSentence, folderName, footerText, runFolder } from "./model.js";
import { button, componentDetail, divider, emptyNote, notesBanner, refreshButton, runStatus, scroller, sectionLabel, staleMark, stateMessage } from "./parts.js";

const MOD_NAME = "cartograph";
const INDENT = 14;
const ROW_PADDING = 12;
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
    this.runs = new Runs(() => this.places.forEach((place) => place.source.clear()));
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

  // Keeps where the node was listed, not the node itself, so a run that rewrites descriptions
  // shows up in the details once its parent level reloads.
  select(place, parentId, node, cx) {
    place.selected = { parentId, node };
    cx.notify();
  }

  selectedNode(place) {
    if (!place.selected) return null;
    const { parentId, node } = place.selected;
    const level = place.source.level(parentId);
    return (level?.status === "ready" ? level.children.find((child) => child.id === node.id) : null) ?? node;
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
      if (depth > 0) rows.push(div().pl(ROW_PADDING + depth * INDENT).py(3).text_size(12).text_color(t.dim).child(level?.status === "loading" || !level ? "Loading…" : level.detail));
      return rows;
    }
    for (const node of level.children) {
      rows.push(this.treeRow(t, place, parentId, node, depth, cx));
      if (place.expanded.has(node.id)) this.treeRows(t, place, node.id, depth + 1, rows, cx);
    }
    if (level.hiddenChildren > 0) rows.push(div().pl(ROW_PADDING + depth * INDENT).py(3).text_size(12).text_color(t.dim).child(`${level.hiddenChildren} more not shown`));
    return rows;
  }

  treeRow(t, place, parentId, node, depth, cx) {
    const below = place.source.level(node.id);
    const isLeaf = below?.status === "ready" && below.children.length === 0;
    const isOpen = place.expanded.has(node.id);
    const isSelected = place.selected?.node.id === node.id;
    const textColor = node.isStale ? t.dim : t.foreground;
    return h_flex().id(`node-${node.id}`).flex_none().pl(ROW_PADDING + depth * INDENT).pr(ROW_PADDING).py(4).gap(4).items_start().cursor_pointer()
      .when(isSelected, (el) => el.bg(t.selected)).hover((s) => s.bg(isSelected ? t.selected : t.hover))
      .on_click((_event, cx) => this.select(place, parentId, node, cx))
      .child(div().id(`toggle-${node.id}`).w(10).flex_none().text_size(10).text_color(t.dim).pt(3)
        // On press, and stopped there, so the row's own click (select) doesn't fire as well.
        .when(!isLeaf, (el) => el.on_mouse_down("left", (_event, cx) => { cx.stop_propagation(); this.toggle(place, node, cx); }))
        .child(isLeaf ? "" : isOpen ? "▾" : "▸"))
      .child(v_flex().flex_1().min_w(0)
        .child(h_flex().gap(5).min_w(0)
          .child(div().min_w(0).truncate().text_size(13).text_color(textColor).child(node.name))
          .when(node.isStale, (el) => el.child(staleMark(t))))
        .when(Boolean(node.summary), (el) => el.child(div().truncate().text_size(12).text_color(t.dim).child(firstSentence(node.summary ?? "")))));
  }

  // One run per graph: the key is the folder the run starts in, wherever in it the person is.
  runTarget(place, where) {
    const folder = runFolder(place.source.info?.root, where.folder);
    return { key: JSON.stringify([where.on, folder]), target: { folder, on: where.on, executable: this.executable } };
  }

  runButton(t, place, where, id, label, commandId, componentId = null) {
    const { key, target } = this.runTarget(place, where);
    const isBusy = this.runs.isRunning(key) || place.source.info?.status !== "ready";
    return button(t, id, label, (_event, cx) => this.runs.start(key, target, commandId, componentId, cx), { isDisabled: isBusy });
  }

  runControls(t, place, where) {
    return h_flex().flex_none().flex_wrap().gap(6).px(12).pb(8)
      .child(this.runButton(t, place, where, "run-update", "Update graph", "update"))
      .child(this.runButton(t, place, where, "run-describe", "Describe", "describe"))
      .child(this.runButton(t, place, where, "run-install", "Install for agents", "install"));
  }

  header(t, place, where, repo) {
    const status = place ? runStatus(t, this.runs.state(this.runTarget(place, where).key)) : null;
    return v_flex().flex_none()
      .child(h_flex().gap(4).items_center().px(12).pt(10).pb(6)
        .child(div().text_size(11).font_semibold().text_color(t.muted).child("CARTOGRAPH"))
        .child(div().flex_1())
        .child(div().min_w(0).truncate().text_size(11).text_color(t.dim).child(repo || "No folder"))
        .when(Boolean(place), (el) => el.child(refreshButton(t, (_event, cx) => this.refresh(place, cx)))))
      .when(Boolean(place), (el) => el.child(this.runControls(t, place, where)))
      .when(Boolean(status), (el) => el.child(status))
      .child(divider(t));
  }

  tree(t, place, top, cx) {
    return v_flex().flex_1().min_h(0)
      .child(sectionLabel(t, "Components", { count: top.children.length, hint: "click to select" }))
      .child(scroller("tree", v_flex().pb(12).children(this.treeRows(t, place, null, 0, [], cx))));
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
    const shell = v_flex().id("cartograph").size_full().min_h(0).bg(t.chrome).text_color(t.foreground).text_size(13);
    if (!where.folder) return shell.child(this.header(t, null, where, "")).child(emptyNote(t, "Focus a space or an agent to see its map."));
    const place = this.place(where);
    this.ensureInfo(place.source, cx);
    const top = place.source.level(null);
    this.ensureLevel(place.source, null, cx);
    const repo = place.source.info?.repo ?? folderName(where.folder);
    const problem = stateMessage(t, top, where.folder);
    const selected = this.selectedNode(place);
    return shell
      .child(this.header(t, place, where, repo))
      .when(Boolean(notesBanner(t, top)), (el) => el.child(notesBanner(t, top)))
      .child(problem ?? this.tree(t, place, top, cx))
      .when(Boolean(problem), (el) => el.child(div().flex_1()))
      .when(Boolean(selected) && !problem, (el) => {
        this.ensureLevel(place.source, place.selected.parentId, cx);
        this.ensureLevel(place.source, selected.id, cx);
        return el.child(v_flex().flex_none().h(DETAIL_HEIGHT).border_t(1).border_color(t.border)
          .child(scroller("detail", v_flex().p(12).gap(10)
            .child(componentDetail(t, selected, place.source.level(selected.id)))
            .child(h_flex().child(this.runButton(t, place, where, "run-describe-this", "Describe this", "describeComponent", selected.id))))));
      })
      .child(this.footer(t, place, where, repo));
  }
}
