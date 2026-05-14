import("./message").then(({ message }) => {
  globalThis.__message = message;
});
