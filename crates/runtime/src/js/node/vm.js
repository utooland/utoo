// Minimal vm module for utoo-runtime

class Script {
  constructor(code, options) {
    this.code = code;
    this.filename = (options && options.filename) || "evalmachine.<anonymous>";
  }

  runInThisContext(options) {
    return (0, eval)(this.code);
  }

  runInNewContext(sandbox, options) {
    const ctx = sandbox || {};
    // Shadow global object references so code sets properties on sandbox
    const fn = new Function("globalThis", "self", "global", "window", this.code);
    return fn.call(ctx, ctx, ctx, ctx, ctx);
  }

  runInContext(context, options) {
    const ctx = context || {};
    const fn = new Function("globalThis", "self", "global", "window", this.code);
    return fn.call(ctx, ctx, ctx, ctx, ctx);
  }
}

function createContext(sandbox) {
  return sandbox || {};
}

function isContext() {
  return false;
}

function runInThisContext(code, options) {
  return new Script(code, options).runInThisContext(options);
}

function runInNewContext(code, sandbox, options) {
  return new Script(code, options).runInNewContext(sandbox, options);
}

function runInContext(code, context, options) {
  return new Script(code, options).runInContext(context, options);
}

function compileFunction(code, params, options) {
  const fn = new Function(...(params || []), code);
  return fn;
}

const vm = {
  Script,
  createContext,
  isContext,
  runInThisContext,
  runInNewContext,
  runInContext,
  compileFunction,
};
vm.default = vm;

export default vm;
export { Script, createContext, isContext, runInThisContext, runInNewContext, runInContext, compileFunction };
