export interface TurbopackUpdateQueue<Update> {
  turbopackUpdates: Update[];
}

export function createHmrTimingReporter(now: () => number = Date.now) {
  let initialBatchFinished = false;
  let batchStartedAt: number | undefined;
  let reportedCurrentBatch = false;

  return {
    start() {
      batchStartedAt = now();
      reportedCurrentBatch = false;
    },
    end() {
      if (!initialBatchFinished) {
        initialBatchFinished = true;
        batchStartedAt = undefined;
        reportedCurrentBatch = false;
      }
    },
    markHmrUpdate() {
      if (
        !initialBatchFinished ||
        batchStartedAt === undefined ||
        reportedCurrentBatch
      ) {
        return undefined;
      }
      reportedCurrentBatch = true;
      return Math.max(0, now() - batchStartedAt);
    },
  };
}

export async function consumeHmrSubscription<Result extends { type: string }>(
  subscription: AsyncIterable<Result>,
  onResult: (result: Result) => void,
  onUpdate: (result: Result) => void,
) {
  for await (const result of subscription) {
    onResult(result);
    if (result.type !== "issues") {
      onUpdate(result);
    }
  }
}

export interface SharedHmrSubscriber<
  Result extends { type: string; issues: unknown[] },
> {
  onIssues: (issues: Result["issues"], resultType: Result["type"]) => void;
  onUpdate: (result: Result) => void;
  onError?: (error: unknown) => void;
  onComplete?: () => void;
}

export function createSharedHmrSubscriptionRegistry<
  Result extends { type: string; issues: unknown[] },
>(createSubscription: (id: string) => AsyncIterableIterator<Result>) {
  type Subscriber = SharedHmrSubscriber<Result>;
  type Member = { active: boolean; subscriber: Subscriber };
  type Entry = {
    latestIssues?: Result["issues"];
    latestResultType?: Result["type"];
    subscription: AsyncIterableIterator<Result>;
    subscribers: Set<Member>;
  };

  const entries = new Map<string, Entry>();

  const reportSubscriberError = (member: Member, error: unknown) => {
    if (!member.active) return;
    try {
      member.subscriber.onError?.(error);
    } catch {
      // A client callback must not terminate the shared project iterator.
    }
  };

  const consume = async (id: string, entry: Entry) => {
    try {
      for await (const result of entry.subscription) {
        entry.latestIssues = [...result.issues] as Result["issues"];
        entry.latestResultType = result.type;
        if (result.type !== "issues" && entries.get(id) === entry) {
          // A late subscriber cannot reuse a stream after its version state has
          // advanced: it would receive the latest issues but miss the update
          // which advanced that state. Keep serving existing subscribers, but
          // force future ones to establish and validate a fresh baseline.
          entries.delete(id);
        }
        for (const member of [...entry.subscribers]) {
          if (!member.active) continue;
          const { subscriber } = member;
          try {
            subscriber.onIssues(
              [...entry.latestIssues] as Result["issues"],
              result.type,
            );
            if (member.active && result.type !== "issues") {
              subscriber.onUpdate(result);
            }
          } catch (error) {
            reportSubscriberError(member, error);
          }
        }
      }
    } catch (error) {
      [...entry.subscribers].forEach((member) =>
        reportSubscriberError(member, error),
      );
    } finally {
      if (entries.get(id) === entry) {
        entries.delete(id);
      }
      entry.subscribers.forEach((member) => {
        member.active = false;
        try {
          member.subscriber.onComplete?.();
        } catch {
          // Completion cleanup is isolated per client.
        }
      });
      entry.subscribers.clear();
    }
  };

  return {
    subscribe(id: string, subscriber: Subscriber) {
      let entry = entries.get(id);
      let start = false;
      if (!entry) {
        entry = {
          subscription: createSubscription(id),
          subscribers: new Set(),
        };
        entries.set(id, entry);
        start = true;
      }
      const member: Member = { active: true, subscriber };
      entry.subscribers.add(member);
      if (entry.latestIssues && entry.latestResultType) {
        try {
          subscriber.onIssues(
            [...entry.latestIssues] as Result["issues"],
            entry.latestResultType,
          );
        } catch (error) {
          reportSubscriberError(member, error);
        }
      }
      if (start) {
        void consume(id, entry);
      }

      return {
        return() {
          if (!member.active) return;

          member.active = false;
          entry.subscribers.delete(member);
          if (entry.subscribers.size > 0) return;

          if (entries.get(id) === entry) {
            entries.delete(id);
          }
          return returnSubscription(entry.subscription);
        },
      };
    },
  };
}

export function enqueueTurbopackUpdateForClient<
  Client extends object,
  Update extends { issues: unknown[] },
>(
  clientStates: WeakMap<Client, TurbopackUpdateQueue<Update>>,
  client: Client,
  payload: Update,
) {
  const update = { ...payload, issues: [] } as Update;
  clientStates.get(client)?.turbopackUpdates.push(update);
}

export interface ReturnableSubscription {
  return?: () => unknown;
}

function returnSubscription(subscription: ReturnableSubscription) {
  try {
    return Promise.resolve(subscription.return?.()).then(
      () => undefined,
      () => undefined,
    );
  } catch {
    return Promise.resolve();
  }
}

export function unsubscribeAllClientSubscriptions<
  Subscription extends ReturnableSubscription,
>(subscriptions: Map<string, Subscription>) {
  const activeSubscriptions = [...subscriptions.values()];
  subscriptions.clear();

  return Promise.all(activeSubscriptions.map(returnSubscription)).then(
    () => undefined,
  );
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
  return returnSubscription(subscription);
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
