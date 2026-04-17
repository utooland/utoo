/**
 * Client-side transport function for server function calls.
 *
 * This module is referenced by `server.functions.callServerModule` in the config.
 * It exports a `callServer` function that frameworks use to dispatch
 * server function calls.
 *
 * In a real application, this would make an HTTP request to the server
 * to invoke the server function.
 */
export async function callServer(actionId: string, args: unknown[]) {
  console.log(`[transport] Calling server function: ${actionId}`, args);

  // In production, this would be something like:
  // const response = await fetch('/__server_fn', {
  //   method: 'POST',
  //   headers: { 'Content-Type': 'application/json' },
  //   body: JSON.stringify({ actionId, args }),
  // });
  // return response.json();

  throw new Error(
    `Server function ${actionId} was called on the client. ` +
      `Configure a server to handle server function calls.`,
  );
}
