import { activePublicPath } from "./public-path.js";
import asset from "./asset.jpg";

export function getImageUrl() {
  return asset;
}

export function getActivePublicPath() {
  return activePublicPath;
}
