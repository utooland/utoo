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
