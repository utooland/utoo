globalThis.__packHmrPageToken ||= crypto.randomUUID();
globalThis.__packHmrApiAvailable = Boolean(import.meta.turbopackHot);
globalThis.__packHmrEntryEvaluations =
  (globalThis.__packHmrEntryEvaluations || 0) + 1;

document.querySelector("#load").onclick = async () => {
  try {
    await Promise.all([import("./a.js"), import("./b.js")]);
  } catch (error) {
    document.querySelector("#error").textContent = String(error);
  }
};

if (import.meta.turbopackHot) {
  import.meta.turbopackHot.accept();
}
