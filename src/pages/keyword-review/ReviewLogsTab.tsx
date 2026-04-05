import { useState } from "react";
import { toast } from "sonner";
import {
  useKeywordReviewLogsClearAllMutation,
  useKeywordReviewLogsQuery,
} from "../../query/keywordReview";
import type { KeywordEvidenceSnippet, KeywordReviewLog } from "../../services/keywordReview";
import { Button } from "../../ui/Button";
import { ConfirmDialog } from "../../ui/ConfirmDialog";
import { cn } from "../../ui/shadcn/utils";

const STATUS_CONFIG: Record<string, { label: string; className: string }> = {
  pending: {
    label: "待审核",
    className: "bg-yellow-100 text-yellow-800 dark:bg-yellow-900/30 dark:text-yellow-300",
  },
  approved: {
    label: "已批准",
    className: "bg-green-100 text-green-800 dark:bg-green-900/30 dark:text-green-300",
  },
  rejected: {
    label: "已拒绝",
    className: "bg-red-100 text-red-800 dark:bg-red-900/30 dark:text-red-300",
  },
  timeout: {
    label: "超时",
    className: "bg-slate-100 text-slate-600 dark:bg-slate-800 dark:text-slate-400",
  },
};

function formatTime(unix: number): string {
  return new Date(unix * 1000).toLocaleString("zh-CN", {
    month: "2-digit",
    day: "2-digit",
    hour: "2-digit",
    minute: "2-digit",
    second: "2-digit",
  });
}

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

function EvidencePreview({ item }: { item: KeywordEvidenceSnippet }) {
  return (
    <div className="mt-1 space-y-0.5 text-xs text-muted-foreground font-mono">
      {item.lines.slice(0, 2).map((line) => (
        <div key={`${item.keyword}_${line.line_number}`} className="flex gap-2">
          <span className="shrink-0 select-none text-[11px] text-muted-foreground">
            L{line.line_number}
          </span>
          <span className="min-w-0 break-all whitespace-pre-wrap">
            {highlightText(line.text, [item.keyword])}
          </span>
        </div>
      ))}
    </div>
  );
}

export function ReviewLogsTab() {
  const [showClearDialog, setShowClearDialog] = useState(false);
  const logsQuery = useKeywordReviewLogsQuery(50, 0);
  const clearLogsMutation = useKeywordReviewLogsClearAllMutation();
  const logs = logsQuery.data ?? [];

  async function handleClearLogs() {
    try {
      const result = await clearLogsMutation.mutateAsync();
      toast(`已清空审核记录：${result.keyword_review_logs_deleted} 条`);
      setShowClearDialog(false);
    } catch (error) {
      toast(error instanceof Error ? error.message : "清空审核记录失败");
    }
  }

  if (logs.length === 0) {
    return <div className="text-sm text-muted-foreground py-8 text-center">暂无审核记录。</div>;
  }

  return (
    <>
      <div className="mb-3 flex items-center justify-end">
        <Button
          variant="danger"
          size="sm"
          onClick={() => setShowClearDialog(true)}
          disabled={clearLogsMutation.isPending || logs.length === 0}
        >
          清空记录
        </Button>
      </div>
      <div className="space-y-2">
        {logs.map((log: KeywordReviewLog) => {
          const statusInfo = STATUS_CONFIG[log.status] ?? STATUS_CONFIG.pending;
          return (
            <div key={log.id} className="flex items-start gap-3 px-3 py-2.5 rounded-md border">
              <div className="flex-1 min-w-0">
                <div className="flex items-center gap-2 mb-1">
                  <span
                    className={cn(
                      "inline-flex items-center rounded px-1.5 py-0.5 text-xs font-medium",
                      statusInfo.className
                    )}
                  >
                    {statusInfo.label}
                  </span>
                  <span className="text-xs text-muted-foreground">{log.cli_key}</span>
                  <span className="text-xs text-muted-foreground">
                    {formatTime(log.created_at)}
                  </span>
                </div>
                <div className="text-sm">
                  匹配关键词：
                  {log.matched_keywords.map((kw: string, i: number) => (
                    <code key={i} className="mx-0.5 px-1 py-0.5 rounded bg-muted text-xs">
                      {kw}
                    </code>
                  ))}
                </div>
                {log.keyword_evidence && log.keyword_evidence.length > 0 ? (
                  <EvidencePreview item={log.keyword_evidence[0]} />
                ) : log.request_snippet ? (
                  <div className="text-xs text-muted-foreground mt-1 line-clamp-2 break-all">
                    {log.request_snippet}
                  </div>
                ) : null}
              </div>
            </div>
          );
        })}
      </div>
      <ConfirmDialog
        open={showClearDialog}
        title="确认清空审核记录"
        description="将删除全部关键词审核历史记录，此操作不可撤销。不会影响当前待审核项和关键词配置。"
        onClose={() => setShowClearDialog(false)}
        onConfirm={() => void handleClearLogs()}
        confirmLabel="确认清空"
        confirmingLabel="清空中…"
        confirming={clearLogsMutation.isPending}
        disabled={logs.length === 0}
        confirmVariant="danger"
      />
    </>
  );
}
