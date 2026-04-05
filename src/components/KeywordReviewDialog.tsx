import { useCallback, useMemo, useState } from "react";
import { type KeywordEvidenceSnippet, type PendingReviewSnapshot } from "../services/keywordReview";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogHeader,
  DialogTitle,
} from "../ui/shadcn/dialog";
import { Button } from "../ui/shadcn/button";
import { useAsyncListener } from "../hooks/useAsyncListener";
import { subscribeGatewayEvent } from "../services/gatewayEventBus";
import { gatewayEventNames } from "../constants/gatewayEvents";
import { useKeywordReviewDecideMutation } from "../query/keywordReview";

type KeywordReviewEvent = PendingReviewSnapshot;

function escapeRegExp(value: string) {
  return value.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");
}

function highlightText(text: string, keywords: string[]) {
  const filtered = [...new Set(keywords.filter(Boolean))].sort((a, b) => b.length - a.length);
  if (!text || filtered.length === 0) return text;

  const pattern = filtered.map((keyword) => escapeRegExp(keyword)).join("|");
  if (!pattern) return text;

  const parts = text.split(new RegExp(`(${pattern})`, "gi"));
  return parts.map((part, index) => {
    const matchedKeyword = filtered.find((keyword) => part.toLowerCase() === keyword.toLowerCase());
    if (!matchedKeyword) {
      return <span key={index}>{part}</span>;
    }
    return (
      <mark
        key={index}
        className="rounded bg-red-200 px-0.5 text-red-950 dark:bg-red-900/60 dark:text-red-100"
      >
        {part}
      </mark>
    );
  });
}

function EvidenceBlock({ item }: { item: KeywordEvidenceSnippet }) {
  const highlightKeywords = useMemo(() => [item.keyword], [item.keyword]);

  return (
    <div className="rounded-md border border-red-200 dark:border-red-900/40 bg-red-50/60 dark:bg-red-950/20 p-2 space-y-1.5">
      <div className="flex items-center justify-between gap-2">
        <div className="flex items-center gap-2 min-w-0">
          <span className="inline-flex items-center rounded px-1.5 py-0.5 text-xs font-medium bg-red-100 text-red-800 dark:bg-red-900/30 dark:text-red-300">
            {item.keyword}
          </span>
          <span className="text-xs text-muted-foreground">命中行 L{item.hit_line_number}</span>
        </div>
      </div>
      <div className="space-y-1 font-mono text-xs">
        {item.lines.map((line) => (
          <div key={`${item.keyword}_${line.line_number}`} className="flex gap-2">
            <span className="shrink-0 select-none text-[11px] text-muted-foreground">
              L{line.line_number}
            </span>
            <span className="min-w-0 whitespace-pre-wrap break-all">
              {highlightText(line.text, highlightKeywords)}
            </span>
          </div>
        ))}
      </div>
    </div>
  );
}

export function KeywordReviewDialog() {
  const [pendingReview, setPendingReview] = useState<KeywordReviewEvent | null>(null);
  const decideMutation = useKeywordReviewDecideMutation();

  useAsyncListener(
    useCallback(async () => {
      const sub = subscribeGatewayEvent<KeywordReviewEvent>(
        gatewayEventNames.keywordReview,
        (payload) => {
          setPendingReview(payload);
        }
      );
      await sub.ready;
      return () => sub.unsubscribe();
    }, []),
    "keyword_review_listener",
    "关键词审核事件监听失败"
  );

  const handleDecide = (decision: "approve" | "reject", allowSession?: boolean) => {
    if (!pendingReview) return;
    decideMutation.mutate(
      { traceId: pendingReview.trace_id, decision, allowSession },
      { onSettled: () => setPendingReview(null) }
    );
  };

  return (
    <Dialog open={pendingReview !== null} onOpenChange={(open) => !open && setPendingReview(null)}>
      <DialogContent className="sm:max-w-2xl">
        <DialogHeader>
          <DialogTitle>请求关键词审核</DialogTitle>
          <DialogDescription>检测到请求中包含敏感关键词，请审核后决定是否放行。</DialogDescription>
        </DialogHeader>

        {pendingReview && (
          <div className="space-y-3">
            <div>
              <span className="text-sm text-muted-foreground">CLI：</span>
              <span className="text-sm font-medium ml-1">{pendingReview.cli_key}</span>
            </div>
            <div>
              <span className="text-sm text-muted-foreground">匹配关键词：</span>
              <div className="flex flex-wrap gap-1 mt-1">
                {pendingReview.matched_keywords.map((kw, i) => (
                  <span
                    key={i}
                    className="inline-flex items-center rounded px-1.5 py-0.5 text-xs font-medium bg-red-100 text-red-800 dark:bg-red-900/30 dark:text-red-300"
                  >
                    {kw}
                  </span>
                ))}
              </div>
            </div>
            {pendingReview.keyword_evidence && pendingReview.keyword_evidence.length > 0 ? (
              <div>
                <span className="text-sm text-muted-foreground">命中位置：</span>
                <div className="mt-1 max-h-64 space-y-2 overflow-auto">
                  {pendingReview.keyword_evidence.map((item) => (
                    <EvidenceBlock key={`${item.keyword}_${item.hit_line_number}`} item={item} />
                  ))}
                </div>
              </div>
            ) : pendingReview.request_snippet ? (
              <div>
                <span className="text-sm text-muted-foreground">内容摘要：</span>
                <pre className="mt-1 p-2 rounded bg-muted text-xs max-h-40 overflow-auto whitespace-pre-wrap break-all">
                  {pendingReview.request_snippet}
                </pre>
              </div>
            ) : null}
          </div>
        )}

        <div className="flex justify-end gap-2 pt-2">
          <Button
            variant="danger"
            onClick={() => handleDecide("reject")}
            disabled={decideMutation.isPending}
          >
            拒绝
          </Button>
          <Button
            variant="ghost"
            onClick={() => handleDecide("approve")}
            disabled={decideMutation.isPending}
          >
            批准放行
          </Button>
          <Button onClick={() => handleDecide("approve", true)} disabled={decideMutation.isPending}>
            本次对话放行
          </Button>
        </div>
      </DialogContent>
    </Dialog>
  );
}
