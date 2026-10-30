export function jsx(type, props) {
  return { factory: "automatic", type, children: [props.children] };
}

export { jsx as jsxs };
