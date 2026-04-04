import { useCallback, useState } from "react";
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

type KeywordReviewEvent = {
  trace_id: string;
  cli_key: string;
  matched_keywords: string[];
  request_snippet: string | null;
  created_at: number;
};

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

  const handleDecide = (decision: "approve" | "reject") => {
    if (!pendingReview) return;
    decideMutation.mutate(
      { traceId: pendingReview.trace_id, decision },
      { onSettled: () => setPendingReview(null) }
    );
  };

  return (
    <Dialog open={pendingReview !== null} onOpenChange={(open) => !open && setPendingReview(null)}>
      <DialogContent className="sm:max-w-lg">
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
            {pendingReview.request_snippet && (
              <div>
                <span className="text-sm text-muted-foreground">内容摘要：</span>
                <pre className="mt-1 p-2 rounded bg-muted text-xs max-h-40 overflow-auto whitespace-pre-wrap break-all">
                  {pendingReview.request_snippet}
                </pre>
              </div>
            )}
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
          <Button onClick={() => handleDecide("approve")} disabled={decideMutation.isPending}>
            批准放行
          </Button>
        </div>
      </DialogContent>
    </Dialog>
  );
}
