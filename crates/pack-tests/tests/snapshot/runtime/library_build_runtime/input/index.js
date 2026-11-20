import icon from "./icon.svg";

console.log('Hello, world!')

export {
  dependencyUrl,
  dependencyEnv,
  readDependencyUrl,
  answer,
} from "./meta space % 中文.mjs";
export { icon };

const meta = import.meta;

export const entryUrl = meta.url;
export const entryEnv = meta.env;
export function readEntryUrl() {
  return meta.url;
}
