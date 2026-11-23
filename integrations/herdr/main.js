// Cartograph's code map in Herdr. Docked, it is the sidebar; opened with props {view: "full"} (the
// sidebar's "Open full view") it is the full map beside the agent's terminal. A `cartograph`
// prop points either at another executable than the one on PATH.
import { View } from "gpui-kit";
import { props } from "herdr";
import { Sidebar } from "./sidebar.js";
import { FullView } from "./full-view.js";

export default class Cartograph extends View {
  init(_props, _cx) {
    const given = props() ?? {};
    this.flavour = given.view === "full" ? new FullView() : new Sidebar();
    this.flavour.init(given);
  }

  render(cx) {
    return this.flavour.render(cx);
  }
}
