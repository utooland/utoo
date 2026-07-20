"use server";

export async function getOrder(orderId: string) {
  return {
    orderId,
    status: "paid" as const,
  };
}
