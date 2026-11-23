export enum Level {
  Info,
  Debug,
}

export default function makeLogger(level: Level) {
  return (message: string) => console.log(level, message);
}
