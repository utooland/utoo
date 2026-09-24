import * as ui from "./outer2";

globalThis.reexportedValues = Object.keys(ui).reduce((values, name) => {
  values[name] = ui[name].value;
  return values;
}, {});
