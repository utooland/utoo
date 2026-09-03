export function h(type: unknown, props: unknown, ...children: unknown[]) {
  return { type, props, children };
}

export const Fragment = Symbol.for("fragment");
