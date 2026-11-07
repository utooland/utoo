import type { ComponentProps } from "react";

// Upstream command primitives import next/link for their optional navigation
// mode. The standalone analyzer uses the action mode and has no router.
export default function StaticLink(props: ComponentProps<"a">) {
  return <a {...props} />;
}
