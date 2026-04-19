/**
 * Client-side transport function for server function calls.
 *
 * This module is referenced by `server.function.clientProxy` in the config.
 * It exports a `createServerReference` factory that framework uses to map
 * server function action stubs across the client bounds.
 *
 * In a real application, this would make an HTTP request to the server
 * to invoke the server function.
 */
export function createServerReference(actionId: string, name: string) {
  return async function (...args: unknown[]) {
    console.log(
      `[transport] Calling server function: ${name} (${actionId})`,
      args,
    );

    // In production, this would be something like:
    // const response = await fetch('/__server_fn', {
    //   method: 'POST',
    //   headers: { 'Content-Type': 'application/json' },
    //   body: JSON.stringify({ actionId, name, args }),
    // });
    // return response.json();

    throw new Error(
      `Server function ${name} (${actionId}) was called on the client. ` +
        `Configure a server to handle server function calls.`,
    );
  };
}
