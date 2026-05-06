// Regression for a production tree-shaking bug where imports from a
// sideEffects-free barrel re-export were dropped while local uses remained.
import { select, visibility } from 'pkg';

export function renderTicks(node) {
  return select(node).append('tick').name;
}

export function applyVisibility(node) {
  visibility(node, true);
  return node.visible;
}
