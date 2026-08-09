const hot = import.meta.turbopackHot;

if (hot) {
  hot.accept();
}

if (module.hot) {
  module.hot.accept();
}

export const value = "initial";
