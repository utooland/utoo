import { types } from "node:util";
import configuredTag from "has-tostringtag/shams";

export function checkArguments() {
  return (function () {
    return types.isArgumentsObject(arguments);
  })();
}

export function configuredTagValue() {
  return configuredTag();
}
