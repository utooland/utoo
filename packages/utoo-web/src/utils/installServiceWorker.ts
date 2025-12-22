import { ServiceWorkerHandShake } from "./message";

export async function installServiceWorker(
  url: string,
  scope: string,
  targetDirToCwd?: string,
) {
  const swUrl = new URL(url, window.location.href);
  swUrl.searchParams.set("scope", scope);
  if (targetDirToCwd) {
    swUrl.searchParams.set("targetDirToCwd", targetDirToCwd);
  }

  const registration = await navigator.serviceWorker.register(
    swUrl.toString(),
    {
      scope: "/",
    },
  );

  return new Promise<void>((resolve) => {
    function sendMessage(sw: ServiceWorker) {
      sw.postMessage({
        [ServiceWorkerHandShake]: true,
      });
      resolve();
    }

    function listenForActivation(sw: ServiceWorker) {
      sw.addEventListener("statechange", () => {
        if (sw.state === "activated") {
          sendMessage(sw);
        }
      });
    }

    function checkSWState(registration: ServiceWorkerRegistration) {
      if (registration.active) {
        sendMessage(registration.active);
      } else if (registration.installing) {
        listenForActivation(registration.installing);
      }

      registration.addEventListener("updatefound", () => {
        if (registration.installing) {
          listenForActivation(registration.installing);
        }
      });
    }

    checkSWState(registration);
  });
}
