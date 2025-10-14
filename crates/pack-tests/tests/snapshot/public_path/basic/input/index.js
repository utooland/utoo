import asset from "./asset.jpg";
console.log('Main entry loaded with publicPath');

export async function loadLazyModule() {
  const module = await import('./lazy.js');
  return module.default();
}

export function getImageUrl() {
  return asset;
}

import('./lazy.js').then(module => {
  console.log(module.default());
});

console.log('Ready to load chunks from CDN');

