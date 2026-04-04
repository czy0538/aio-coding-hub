import { useKeywordReviewLogsQuery } from "../../query/keywordReview";
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

export function ReviewLogsTab() {
  const logsQuery = useKeywordReviewLogsQuery(50, 0);
  const logs = logsQuery.data ?? [];

  if (logs.length === 0) {
    return <div className="text-sm text-muted-foreground py-8 text-center">暂无审核记录。</div>;
  }

  return (
    <div className="space-y-2">
      {logs.map((log) => {
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
                <span className="text-xs text-muted-foreground">{formatTime(log.created_at)}</span>
              </div>
              <div className="text-sm">
                匹配关键词：
                {log.matched_keywords.map((kw, i) => (
                  <code key={i} className="mx-0.5 px-1 py-0.5 rounded bg-muted text-xs">
                    {kw}
                  </code>
                ))}
              </div>
              {log.request_snippet && (
                <div className="text-xs text-muted-foreground mt-1 line-clamp-2 break-all">
                  {log.request_snippet}
                </div>
              )}
            </div>
          </div>
        );
      })}
    </div>
  );
}
