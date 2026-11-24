// The cartograph runs started from the sidebar: what each folder is running now and how its last
// run ended. Only the folder's key gates a run, so two folders can run side by side.
import * as cli from "./cli.js";
import { runRefreshesTree, runSteps } from "./model.js";

export class Runs {
  /** @param {() => void} onFinished called after a run ends, to reload what the graph changed */
  constructor(onFinished) {
    this.onFinished = onFinished;
    /** @type {Map<string, { stepLabel: string | null, outcome: { isFailure: boolean, text: string } | null }>} */
    this.states = new Map();
  }

  state(key) {
    return this.states.get(key) ?? null;
  }

  isRunning(key) {
    return Boolean(this.state(key)?.stepLabel);
  }

  /** Runs the command's steps in order and stops at the first that fails. Call from a click handler.
   *  @param {import("./cli.js").Target} target */
  start(key, target, commandId, componentId, cx) {
    if (this.isRunning(key)) return;
    const steps = runSteps(commandId, componentId);
    // Marked running before the spawn so a second click in the same frame is turned away.
    this.states.set(key, { stepLabel: steps[0].label, outcome: null });
    cx.spawn(async (cx) => {
      let outcome = null;
      for (const step of steps) {
        this.states.set(key, { stepLabel: step.label, outcome: null });
        cx.notify();
        outcome = await cli.runStep(target, step);
        if (outcome.isFailure) break;
      }
      this.states.set(key, { stepLabel: null, outcome });
      if (runRefreshesTree(commandId)) this.onFinished();
      cx.notify();
    });
  }
}
