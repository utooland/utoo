const worker = new Worker(new URL("./repro.worker.js", import.meta.url));

worker.addEventListener("message", (event) => {
  console.log(event.data);
});
