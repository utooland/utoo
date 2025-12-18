import { promises } from "./fsPolyfill";

export const readFile = promises.readFile;
export const writeFile = promises.writeFile;
export const readdir = promises.readdir;
export const mkdir = promises.mkdir;
export const rm = promises.rm;
export const rmdir = promises.rmdir;
export const copyFile = promises.copyFile;

export default promises;
