import path from "node:path";
import { greet } from "./utils";

const name = process.env.USER ?? "world";
const message = greet(name);

console.log(message);
console.log("cwd:", process.cwd());
console.log("dirname:", path.dirname("/foo/bar/baz.txt"));
console.log("extname:", path.extname("index.ts"));
console.log("joined:", path.join("src", "utils", "index.ts"));
