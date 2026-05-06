import supportsColor, { createSupportsColor } from "supports-color";
import JSON5 from "json5";

const stdout = createSupportsColor({ isTTY: true });
const parsed = JSON5.parse("{feature:'interop'}");

console.log("supports-color", typeof createSupportsColor, supportsColor.stdout.level);
console.log("supports-color stdout", stdout.level);
console.log("json5 default", parsed.feature);
