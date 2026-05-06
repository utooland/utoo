import * as Shape from './index.js';
import { refreshElement } from '../util/draw.js';

export default class Base {
  getShapeBase() {
    return Shape;
  }

  refresh() {
    refreshElement(this);
  }
}
