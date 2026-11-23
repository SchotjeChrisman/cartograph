import { Shape } from "./shape";

export class Circle extends Shape {
  constructor(private radius: number) {
    super();
  }

  area(): number {
    return this.radius * this.radius * this.scale();
  }

  render(): string {
    return `circle ${this.describe()}`;
  }
}
