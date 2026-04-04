export const gatewayEventNames = {
  status: "gateway:status",
  requestStart: "gateway:request_start",
  attempt: "gateway:attempt",
  request: "gateway:request",
  log: "gateway:log",
  circuit: "gateway:circuit",
  keywordReview: "gateway:keyword_review",
} as const;

export type GatewayEventName = (typeof gatewayEventNames)[keyof typeof gatewayEventNames];
