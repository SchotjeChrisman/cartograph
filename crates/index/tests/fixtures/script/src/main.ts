import { Circle, Square } from "./shapes";
import makeLogger, { Level } from "./logger";
import * as math from "./math";
import { missing } from "./not_there";
import express from "express";

const log = makeLogger(Level.Info);

export function run(): number {
  const circle = new Circle(2);
  log(circle.describe());
  return math.double(circle.area()) + helper();
}

function helper(): number {
  return new Square(1).area();
}

// `render` exists on both shapes, and nothing says which one `thing` is.
export function draw(thing: any): void {
  thing.render();
}

missing();
express();
