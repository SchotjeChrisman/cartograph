// The toggle flavour: a second copy of the mod, docked in the status bar, that only shows and hides
// the sidebar panel. It loads nothing.
import { svg } from "gpui-kit";
import { h_flex } from "gpui-base";
import { layout, run, theme } from "herdr";

const ICON = "mods/cartograph/cartograph.svg";

const findNode = (node, matches) => (matches(node) ? node : (node?.children ?? []).map((child) => findNode(child, matches)).find(Boolean));

// A "cartograph-panel" column carries the border and radius, so it hides and shows as one; without
// it, the cartograph block alone. The toggle is a cartograph block too (props.toggle), so it is
// skipped; the full view opens as a tab, outside the layout, so it never matches either.
const PANEL_GROUP = "cartograph-panel";
const panelNode = () => findNode(layout(), (node) => node?.id === PANEL_GROUP)
  ?? findNode(layout(), (node) => node?.content?.mod === "cartograph" && !node.content.props?.toggle);

export class Toggle {
  init(_props) {}

  render(_cx) {
    const t = theme();
    const panel = panelNode();
    const isShown = !panel?.hidden;
    return h_flex().size_full().items_center().justify_end().pr(10).bg(t.chrome).border_b(1).border_color(t.border)
      .child(h_flex().id("cartograph-toggle").h(26).px(8).gap(5).items_center().rounded(6).text_size(12)
        .bg(isShown ? t.selected : t.chrome).text_color(isShown ? t.foreground : t.muted).hover((s) => s.bg(t.hover))
        .on_mouse_down("left", (_event, cx) => cx.stop_propagation())
        .on_click(() => { if (panel) run("layout.toggle", { id: panel.id }); })
        .child(svg(ICON).size(14)).child("Cartograph"));
  }
}
