export interface Drawable {
  render(): string;
}

export abstract class Shape implements Drawable {
  abstract area(): number;
  abstract render(): string;

  describe(): string {
    return `area ${this.area()}`;
  }

  protected scale(): number {
    return 3;
  }
}
