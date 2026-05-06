import { Path } from '../shape/index.js';

export function getArrowShape(element) {
  return new Path(element).kind();
}
