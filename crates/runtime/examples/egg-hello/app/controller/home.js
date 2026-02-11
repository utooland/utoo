'use strict';

const { Controller } = require('egg');

class HomeController extends Controller {
  async index() {
    this.ctx.body = 'Hello from Egg.js on utoo-runtime!';
  }
}

module.exports = HomeController;
