if (import.meta.turbopackHot !== undefined) {
  throw new Error("import.meta.turbopackHot should be disabled");
}

if (module.hot !== undefined) {
  throw new Error("module.hot should be disabled");
}

export const value = "initial";
