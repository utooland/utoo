export interface TurbopackUpdateQueue<Update> {
  turbopackUpdates: Update[];
}

export function enqueueTurbopackUpdateForClient<
  Client extends object,
  Update extends { issues: unknown[] },
>(
  clientStates: WeakMap<Client, TurbopackUpdateQueue<Update>>,
  client: Client,
  payload: Update,
) {
  payload.issues = [];
  clientStates.get(client)?.turbopackUpdates.push(payload);
}

export interface ReturnableSubscription {
  return?: () => unknown;
}

export function unsubscribeClient<Subscription extends ReturnableSubscription>(
  subscriptions: Map<string, Subscription>,
  id: string,
) {
  const subscription = subscriptions.get(id);
  if (!subscription) {
    return;
  }

  subscriptions.delete(id);
  subscription.return?.();
}

export function deleteClientSubscriptionIfCurrent<Subscription>(
  subscriptions: Map<string, Subscription>,
  id: string,
  subscription: Subscription,
) {
  if (subscriptions.get(id) === subscription) {
    subscriptions.delete(id);
    return true;
  }

  return false;
}

export function isCurrentClientSubscription<
  Client extends object,
  State extends { subscriptions: ReadonlyMap<string, unknown> },
>(
  clientStates: WeakMap<Client, State>,
  client: Client,
  state: State,
  id: string,
  subscription: unknown,
) {
  return (
    clientStates.get(client) === state &&
    state.subscriptions.get(id) === subscription
  );
}
