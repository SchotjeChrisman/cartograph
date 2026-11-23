import { Shape } from "./shape";

export class Square extends Shape {
  constructor(private side: number) {
    super();
  }

  area(): number {
    return this.side * this.side;
  }

  render(): string {
    return "square";
  }
}
