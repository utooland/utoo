import { makeArrow } from '../util/arrow.js';

export default class Base {
  constructor() {
    this.type = 'base';
  }
  getArrow() {
    return makeArrow(this);
  }
}
