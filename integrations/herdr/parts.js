// Shared UI pieces: buttons, state messages and a component's details for both views, and
// studio-style scroller, divider, section label and empty note for the sidebar.
import { div, svg } from "gpui-kit";
import { v_flex, h_flex, Scrollbar } from "gpui-base";
import { settings } from "herdr";
import { overviewCommand } from "./model.js";

const monospace = () => String(settings().font_family ?? "Menlo");

const REFRESH_ICON = "mods/cartograph/refresh.svg";
const SPINNER_ICON = "mods/cartograph/spinner.svg";
const SPIN_MS = 900;
const OUTCOME_MAX_HEIGHT = 110;

export function button(t, id, label, onClick, { isPrimary = false, isDisabled = false } = {}) {
  const isSolid = isPrimary && !isDisabled;
  return div().id(id).flex_none().h(26).px(8).rounded(6).flex().items_center().text_size(12)
    .when(isSolid, (el) => el.bg(t.solid).text_color(t.solid_foreground).hover((s) => s.bg(t.accent).text_color(t.accent_foreground)))
    .when(!isSolid, (el) => el.border_1().border_color(t.border).text_color(isDisabled ? t.dim : t.foreground))
    .when(!isDisabled, (el) => el.cursor_pointer().on_click(onClick))
    .when(!isDisabled && !isSolid, (el) => el.hover((s) => s.bg(t.hover)))
    .child(label);
}

export function refreshButton(t, onClick) {
  return div().id("refresh").w(22).h(22).flex_none().flex().items_center().justify_center().rounded(4)
    .cursor_pointer().hover((s) => s.bg(t.hover)).tooltip("Refresh").on_click(onClick)
    .child(svg(REFRESH_ICON).size(12).text_color(t.muted));
}

/** What the last run is doing or said: a spinner and the step, or the result (all of it on failure). */
export function runStatus(t, state) {
  if (!state) return null;
  if (state.stepLabel) {
    return h_flex().gap(6).items_center().px(12).pb(6)
      .child(svg(SPINNER_ICON).size(12).text_color(t.muted).spin(SPIN_MS))
      .child(div().text_size(12).text_color(t.muted).child(state.stepLabel));
  }
  const { isFailure, text } = state.outcome;
  return div().id("run-outcome").flex_none().max_h(OUTCOME_MAX_HEIGHT).overflow_y_scroll().px(12).pb(6)
    .child(div().text_size(11).text_color(isFailure ? t.blocked : t.dim).whitespace_normal().child(text));
}

// The scroll area wraps its element, so the content sits absolutely inside a box that never grows
// past its place, with the bar drawn over it.
export const scroller = (id, inner) => div().flex_1().min_h(0).relative()
  .child(inner.id(id).absolute().inset_0().overflow_y_scroll())
  .child(Scrollbar.vertical(id).absolute().inset_0());

export const divider = (t) => div().h(1).mx(12).flex_none().bg(t.border);

const countPill = (t, count) => div().flex_none().px(5).rounded(4).bg(t.hover).text_color(t.muted).text_size(10.5).font_semibold().child(String(count));

export function sectionLabel(t, title, { count = null, hint = "" } = {}) {
  return h_flex().flex_none().gap(6).items_center().px(12).pt(12).pb(6)
    .child(div().text_size(11).font_semibold().text_color(t.muted).child(title.toUpperCase()))
    .when(count !== null, (el) => el.child(countPill(t, count)))
    .child(div().flex_1())
    .child(div().text_size(11).text_color(t.dim).child(hint));
}

export function commandLine(t, command) {
  return div().px(8).py(5).rounded(6).bg(t.background).border_1().border_color(t.border)
    .font_family(monospace()).text_size(11).text_color(t.foreground).child(command);
}

const heading = (t, text) => div().mt(10).mb(4).text_size(11).font_semibold().text_color(t.muted).child(text.toUpperCase());

export const emptyNote = (t, text) => div().px(12).py(4).text_size(12).text_color(t.dim).whitespace_normal().child(text);

export const staleMark = (t) => div().flex_none().text_size(11).text_color(t.dim).child("(stale)");

function message(t, title, lines, command, titleColor = t.foreground) {
  return v_flex().p(12).gap(8).text_size(13)
    .child(div().font_semibold().text_color(titleColor).child(title))
    .children(lines.map((line) => div().text_color(t.muted).line_height("19px").whitespace_normal().child(line)))
    .when(Boolean(command), (el) => el.child(commandLine(t, command)));
}

/** What to show instead of the tree or the boxes, or null when the level is ready to draw. */
export function stateMessage(t, level, folder) {
  if (!level || level.status === "loading") return message(t, "Loading the map…", []);
  if (level.status === "not-installed") {
    return message(t, "cartograph is not on PATH", [`Could not start "${level.executable}".`, level.detail, "Install cartograph, or give this mod a `cartograph` prop (or localStorage key) with the executable's path."]);
  }
  if (level.status === "no-graph") {
    return message(t, "No cartograph graph here yet", [`Nothing indexed at or above ${folder}. Run this in the project's root:`], `cd ${folder} && cartograph index`);
  }
  if (level.status === "failed") return message(t, "cartograph failed", [level.detail], null, t.blocked);
  if (level.isEmpty && level.isIndexing) return message(t, "Indexing in progress…", ["The first index is still running; refresh when it is done."]);
  if (level.isEmpty) return message(t, "The graph is empty", ["Run the index in the project's root:"], `cd ${folder} && cartograph index`);
  if (!level.parent && level.children.length === 0) {
    return message(t, "No components yet", ["The code is indexed but not grouped. Run:"], "cartograph cluster && cartograph describe");
  }
  return null;
}

/** Freshness notes worth a line above the content: indexing, re-indexing, stale warnings. */
export function notesBanner(t, level) {
  if (level?.status !== "ready" || level.notes.length === 0) return null;
  return v_flex().px(12).py(6).gap(2).border_b(1).border_color(t.border).text_size(11)
    .children(level.notes.map((note) => div().text_color(note.startsWith("warning") ? t.blocked : t.muted).whitespace_normal().child(note)));
}

function symbolRow(t, symbol) {
  return v_flex().py(2)
    .child(h_flex().gap(6).min_w(0)
      .child(div().flex_none().text_size(10).text_color(t.dim).child(symbol.kind))
      .child(div().flex_1().min_w(0).truncate().text_size(12).text_color(t.foreground).child(symbol.name)))
    .when(Boolean(symbol.location), (el) => el.child(div().truncate().text_size(10).text_color(t.dim).child(symbol.location)));
}

function linkRow(t, link) {
  const arrows = [link.outgoing ? `uses ${link.outgoing}` : "", link.incoming ? `used by ${link.incoming}` : ""].filter(Boolean).join(" · ");
  return h_flex().py(2).gap(6).text_size(12)
    .child(div().flex_1().min_w(0).truncate().text_color(t.foreground).child(link.name))
    .child(div().flex_none().text_size(11).text_color(t.dim).child(arrows));
}

/** The strongest links section; `links` is undefined before they are asked for. */
export function linksSection(t, links) {
  const body = !links || links.status === "loading"
    ? [div().text_size(12).text_color(t.dim).child("Following the key symbols…")]
    : links.length === 0
      ? [div().text_size(12).text_color(t.dim).child("No links outside this component from its key symbols.")]
      : links.map((link) => linkRow(t, link));
  return v_flex().child(heading(t, "Strongest links")).children(body)
    .child(div().mt(2).text_size(10).text_color(t.dim).whitespace_normal().child("Counted from the direct neighbours of its key symbols."));
}

/** A component's description, tags, key symbols and command. `level` is the level below it. */
export function componentDetail(t, node, level) {
  const symbols = level?.status === "ready" ? level.keySymbols : [];
  return v_flex().gap(2)
    .child(h_flex().gap(6).items_center()
      .child(div().flex_1().min_w(0).font_semibold().text_size(13).text_color(node.isStale ? t.muted : t.foreground).child(node.name))
      .when(node.isStale, (el) => el.child(staleMark(t))))
    .child(div().text_size(12).text_color(node.summary ? t.muted : t.dim).whitespace_normal()
      .child(node.summary ?? "No description yet: run `cartograph describe`."))
    .when(node.tags.length > 0, (el) => el.child(div().text_size(11).text_color(t.dim).child(node.tags.join(" · "))))
    .child(heading(t, "Key symbols"))
    .children(level?.status === "ready"
      ? (symbols.length ? symbols.map((symbol) => symbolRow(t, symbol)) : [div().text_size(12).text_color(t.dim).child("None listed.")])
      : [div().text_size(12).text_color(t.dim).child("Loading…")])
    .child(heading(t, "Command"))
    .child(commandLine(t, overviewCommand(node.id)));
}
