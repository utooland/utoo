/** @jsx factory */
/** @jsxFrag fragment */
const fragment = "custom-fragment";
function factory(type, props, ...children) {
  return { factory: "custom", type, children };
}

export const custom = <>custom pragma</>;
