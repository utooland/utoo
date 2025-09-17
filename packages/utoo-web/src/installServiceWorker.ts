import { ServiceWorkerHandShake } from "./message";

export async function installServiceWorker(url: string, scope: string) {
  const registration = await navigator.serviceWorker.register(url, {
    scope: "/",
  });

  return new Promise<void>((resolve) => {
    function sendMessage(sw: ServiceWorker) {
      sw.postMessage({
        [ServiceWorkerHandShake]: true,
        scope,
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
