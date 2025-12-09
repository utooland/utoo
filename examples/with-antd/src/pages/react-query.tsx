import {
  QueryClient,
  QueryClientProvider,
  useQuery,
} from "@tanstack/react-query";
import React from "react";

export function ReactQueryInternal() {
  // fetch https://jsonplaceholder.typicode.com/posts/1
  const { data: queryData, isLoading } = useQuery({
    queryKey: ["posts", "1"],
    queryFn: () => {
      return fetch("https://jsonplaceholder.typicode.com/posts/1").then((res) =>
        res.json(),
      );
    },
  });

  if (!queryData) return <div>Loading...</div>;

  return (
    <div>
      <h2>React Query</h2>
      {<pre>{JSON.stringify(queryData, null, 2)}</pre>}
    </div>
  );
}

const client = new QueryClient();

export function ReactQuery() {
  return (
    <QueryClientProvider client={client}>
      <ReactQueryInternal />
    </QueryClientProvider>
  );
}
