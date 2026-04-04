import { invokeServiceCommand } from "./invokeServiceCommand";

// ── Types ──

export type KeywordEntry = {
  id: number;
  keyword: string;
  enabled: boolean;
  created_at: number;
  updated_at: number;
};

export type KeywordReviewLog = {
  id: number;
  trace_id: string;
  cli_key: string;
  session_id: string | null;
  matched_keywords: string[];
  request_snippet: string | null;
  status: "pending" | "approved" | "rejected" | "timeout";
  reviewer_action_at: number | null;
  created_at: number;
};

export type PendingReviewSnapshot = {
  trace_id: string;
  cli_key: string;
  matched_keywords: string[];
  request_snippet: string | null;
  created_at: number;
};

// ── Service Functions ──

export function keywordReviewKeywordsList(): Promise<KeywordEntry[]> {
  return invokeServiceCommand({
    title: "获取关键词列表失败",
    cmd: "keyword_review_keywords_list",
  });
}

export function keywordReviewKeywordAdd(keyword: string): Promise<KeywordEntry> {
  return invokeServiceCommand({
    title: "添加关键词失败",
    cmd: "keyword_review_keyword_add",
    args: { keyword },
  });
}

export function keywordReviewKeywordSetEnabled(
  id: number,
  enabled: boolean
): Promise<KeywordEntry> {
  return invokeServiceCommand({
    title: "更新关键词状态失败",
    cmd: "keyword_review_keyword_set_enabled",
    args: { id, enabled },
  });
}

export function keywordReviewKeywordDelete(id: number): Promise<boolean> {
  return invokeServiceCommand({
    title: "删除关键词失败",
    cmd: "keyword_review_keyword_delete",
    args: { id },
  });
}

export function keywordReviewLogsList(limit: number, offset: number): Promise<KeywordReviewLog[]> {
  return invokeServiceCommand({
    title: "获取审核日志失败",
    cmd: "keyword_review_logs_list",
    args: { limit, offset },
  });
}

export function keywordReviewDecide(
  traceId: string,
  decision: "approve" | "reject"
): Promise<boolean> {
  return invokeServiceCommand({
    title: "审核操作失败",
    cmd: "keyword_review_decide",
    args: { traceId, decision },
  });
}

export function keywordReviewPendingList(): Promise<PendingReviewSnapshot[]> {
  return invokeServiceCommand({
    title: "获取待审核列表失败",
    cmd: "keyword_review_pending_list",
  });
}
