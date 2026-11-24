// The full-view flavour: one level of the map as boxes, a breadcrumb to go back up, and a detail
// panel that can hand the component to the agent whose terminal sits beside this pane.
import { div } from "gpui-kit";
import { v_flex, h_flex } from "gpui-base";
import { hc, theme } from "herdr";
import { executable } from "./cli.js";
import { GraphSource } from "./source.js";
import { askPrompt, folderName } from "./model.js";
import { button, commandLine, refreshButton, componentDetail, linksSection, notesBanner, staleMark, stateMessage } from "./parts.js";

const BOX_WIDTH = 240;
const BOX_HEIGHT = 112;
const PANEL_WIDTH = 340;

export class FullView {
  init(props) {
    this.folder = props.folder;
    this.terminal = props.terminal ?? null;
    this.source = new GraphSource({ folder: props.folder, on: props.on ?? null, executable: executable(props) });
    /** The components drilled into, top first; empty at the top level. */
    this.path = [];
    this.sendResult = null;
  }

  get current() {
    return this.path.at(-1) ?? null;
  }

  ensure(load, isLoaded, cx) {
    if (isLoaded) return;
    cx.spawn(async (cx) => {
      await load();
      cx.notify();
    });
  }

  goTo(path, cx) {
    this.path = path;
    this.sendResult = null;
    cx.notify();
  }

  refresh(cx) {
    this.source.clear();
    cx.notify();
  }

  ask(node, cx) {
    const terminal = this.terminal;
    this.sendResult = { text: `Sending to ${terminal}…` };
    cx.notify();
    cx.spawn(async (cx) => {
      try {
        const result = await hc(["send", terminal, askPrompt(node)]);
        this.sendResult = result.code === 0 ? { text: `Sent to ${terminal}.` } : { text: result.stderr.trim() || `hc send exited with ${result.code}`, isError: true };
      } catch (error) {
        this.sendResult = { text: String(error instanceof Error ? error.message : error), isError: true };
      }
      cx.notify();
    });
  }

  breadcrumb(t, repo) {
    const crumbs = [{ name: repo, path: [] }, ...this.path.map((node, index) => ({ name: node.name, path: this.path.slice(0, index + 1) }))];
    return h_flex().flex_1().min_w(0).gap(6).items_center().text_size(13).overflow_hidden()
      .children(crumbs.flatMap((crumb, index) => {
        const isLast = index === crumbs.length - 1;
        const label = div().id(`crumb-${index}`).flex_none().text_color(isLast ? t.foreground : t.muted).when(isLast, (el) => el.font_semibold())
          .when(!isLast, (el) => el.cursor_pointer().hover((s) => s.text_color(t.foreground)).on_click((_event, cx) => this.goTo(crumb.path, cx)))
          .child(crumb.name);
        return index === 0 ? [label] : [div().flex_none().text_color(t.dim).child("›"), label];
      }));
  }

  componentBox(t, node) {
    return v_flex().id(`box-${node.id}`).w(BOX_WIDTH).h(BOX_HEIGHT).p(10).gap(4).rounded(8).overflow_hidden()
      .bg(t.surface).border_1().border_color(t.border).cursor_pointer().hover((s) => s.border_color(t.accent))
      .on_click((_event, cx) => this.goTo([...this.path, node], cx))
      .child(h_flex().gap(6).items_center().min_w(0)
        .child(div().min_w(0).truncate().font_semibold().text_size(13).text_color(node.isStale ? t.dim : t.foreground).child(node.name))
        .when(node.isStale, (el) => el.child(staleMark(t))))
      .child(div().text_size(11).whitespace_normal().text_color(node.isStale ? t.dim : t.muted).child(node.summary ?? "No description yet."));
  }

  boxes(t, level) {
    if (level.children.length === 0) {
      return div().p(16).text_size(12).text_color(t.dim).child(`${this.current.name} has no sub-components; its key symbols are in the panel.`);
    }
    return v_flex().gap(10).p(16)
      .child(h_flex().flex_wrap().gap(12).children(level.children.map((node) => this.componentBox(t, node))))
      .when(level.hiddenChildren > 0, (el) => el.child(div().text_size(11).text_color(t.dim).child(`${level.hiddenChildren} more not shown`)));
  }

  askControls(t, node) {
    const hint = this.terminal ? `Types a question about ${node.name} into ${this.terminal}.` : "No agent was focused when this view opened, so there is no terminal to ask.";
    return v_flex().mt(12).gap(6)
      .child(button(t, "ask", "Ask agent about this", (_event, cx) => this.ask(node, cx), { isPrimary: true, isDisabled: !this.terminal }))
      .child(div().text_size(11).whitespace_normal().text_color(this.sendResult?.isError ? t.blocked : t.dim).child(this.sendResult?.text ?? hint));
  }

  panel(t, level, cx) {
    const node = this.current;
    const shell = v_flex().id("panel").flex_none().w(PANEL_WIDTH).h_full().overflow_y_scrollbar().p(14).border_l(1).border_color(t.border).bg(t.background);
    if (!node) {
      return shell.gap(6)
        .child(div().font_semibold().child("Top level"))
        .child(div().text_size(12).text_color(t.muted).whitespace_normal().child("Click a box to drill in; the breadcrumb goes back up."))
        .child(commandLine(t, "cartograph overview"));
    }
    if (level?.status === "ready") this.ensure(() => this.source.loadLinks(node.id), this.source.linksOf(node.id) !== undefined, cx);
    return shell
      .child(componentDetail(t, node, level))
      .child(linksSection(t, this.source.linksOf(node.id)))
      .child(this.askControls(t, node));
  }

  render(cx) {
    const t = theme();
    const level = this.source.level(this.current?.id ?? null);
    this.ensure(() => this.source.loadLevel(this.current?.id ?? null), level !== undefined, cx);
    this.ensure(() => this.source.loadInfo(), Boolean(this.source.info), cx);
    const repo = this.source.info?.repo ?? folderName(this.folder);
    const problem = stateMessage(t, level, this.folder);
    const banner = notesBanner(t, level);
    return v_flex().id("cartograph-full").size_full().min_h(0).bg(t.background).text_color(t.foreground)
      .child(h_flex().h(42).flex_none().px(14).gap(10).items_center().border_b(1).border_color(t.border)
        .child(this.breadcrumb(t, repo))
        .child(refreshButton(t, (_event, cx) => this.refresh(cx))))
      .when(Boolean(banner), (el) => el.child(banner))
      .child(h_flex().flex_1().min_h(0)
        .child(v_flex().id("boxes").flex_1().min_w(0).h_full().overflow_y_scrollbar().bg(t.canvas).child(problem ?? this.boxes(t, level)))
        .when(!problem, (el) => el.child(this.panel(t, level, cx))));
  }
}
