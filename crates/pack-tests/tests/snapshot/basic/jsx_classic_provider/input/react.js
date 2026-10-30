function createReact(factory) {
  return {
    Fragment: `${factory}-fragment`,
    createElement(type, props, ...children) {
      return { factory, type, children };
    },
  };
}

export const providedReact = createReact("provider");
export const importedReact = createReact("imported");
