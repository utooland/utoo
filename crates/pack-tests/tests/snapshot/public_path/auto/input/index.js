import asset from "./asset.jpg";

export async function loadLazyModule() {
  const module = await import("./lazy.js");
  return module.default();
}

export function getImageUrl() {
  return asset;
}

import("./lazy.js").then((module) => {
  console.log(module.default());
});
