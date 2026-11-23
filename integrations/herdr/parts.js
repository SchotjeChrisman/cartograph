// The pieces both views are built from: buttons, state messages and a component's details.
import { div } from "gpui-kit";
import { v_flex, h_flex } from "gpui-base";
import { settings } from "herdr";
import { overviewCommand } from "./model.js";

const monospace = () => String(settings().font_family ?? "Menlo");

export function button(t, id, label, onClick, { isPrimary = false, isDisabled = false } = {}) {
  const isSolid = isPrimary && !isDisabled;
  const background = isSolid ? t.solid : t.raised;
  return div().id(id).flex_none().px(10).py(4).rounded(6).text_size(12).border_1().border_color(t.border)
    .bg(background).text_color(isDisabled ? t.dim : isSolid ? t.solid_foreground : t.foreground)
    .when(!isDisabled, (el) => el.cursor_pointer().hover((s) => s.bg(isSolid ? background : t.hover)).on_click(onClick))
    .child(label);
}

export function commandLine(t, command) {
  return div().px(8).py(5).rounded(6).bg(t.background).border_1().border_color(t.border)
    .font_family(monospace()).text_size(11).text_color(t.foreground).child(command);
}

const heading = (t, text) => div().mt(10).mb(4).text_size(11).font_semibold().text_color(t.muted).child(text.toUpperCase());

export const staleMark = (t) => div().flex_none().text_size(11).text_color(t.dim).child("(stale)");

function message(t, title, lines, command) {
  return v_flex().p(12).gap(6).text_size(12)
    .child(div().font_semibold().text_color(t.foreground).child(title))
    .children(lines.map((line) => div().text_color(t.muted).whitespace_normal().child(line)))
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
  if (level.status === "failed") return message(t, "cartograph failed", [level.detail]);
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
  return v_flex().px(10).py(6).gap(2).bg(t.raised).border_b(1).border_color(t.border).text_size(11)
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
