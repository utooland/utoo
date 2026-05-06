import { applyVisibility, renderTicks } from './component';

const node = { children: [], visible: false };

console.log(renderTicks(node), applyVisibility(node));
