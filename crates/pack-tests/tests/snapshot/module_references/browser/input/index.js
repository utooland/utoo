import DefaultGreeting, { Greeting } from "./greeting.ts";
import { calls, creations } from "./runtime.js";

export const aliasesMatch = DefaultGreeting === Greeting;
export const instance = new Greeting();
export { calls, creations };
