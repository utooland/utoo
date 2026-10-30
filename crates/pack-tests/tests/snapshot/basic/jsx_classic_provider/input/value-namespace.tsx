namespace React {
  export function createElement(type, props, ...children) {
    return { factory: "namespace", type, children };
  }
}

export const valueNamespace = <section>value namespace</section>;
