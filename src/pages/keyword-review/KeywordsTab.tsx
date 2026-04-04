import { useState } from "react";
import { Button } from "../../ui/shadcn/button";
import { Input } from "../../ui/shadcn/input";
import { Switch } from "../../ui/shadcn/switch";
import { Trash2 } from "lucide-react";
import {
  useKeywordReviewKeywordsQuery,
  useKeywordReviewKeywordAddMutation,
  useKeywordReviewKeywordSetEnabledMutation,
  useKeywordReviewKeywordDeleteMutation,
} from "../../query/keywordReview";
import { useSettingsQuery, useSettingsSetMutation } from "../../query/settings";

export function KeywordsTab() {
  const [newKeyword, setNewKeyword] = useState("");
  const keywordsQuery = useKeywordReviewKeywordsQuery();
  const addMutation = useKeywordReviewKeywordAddMutation();
  const setEnabledMutation = useKeywordReviewKeywordSetEnabledMutation();
  const deleteMutation = useKeywordReviewKeywordDeleteMutation();
  const settingsQuery = useSettingsQuery();
  const settingsMutation = useSettingsSetMutation();

  const settings = settingsQuery.data;
  const isEnabled = settings?.enable_keyword_review ?? false;
  const timeoutSeconds = settings?.keyword_review_timeout_seconds ?? 300;
  const timeoutAction = settings?.keyword_review_timeout_action ?? "reject";

  const handleAdd = () => {
    const trimmed = newKeyword.trim();
    if (!trimmed) return;
    addMutation.mutate(trimmed, {
      onSuccess: () => setNewKeyword(""),
    });
  };

  const handleToggleEnabled = (checked: boolean) => {
    if (!settings) return;
    settingsMutation.mutate({
      preferredPort: settings.preferred_port,
      autoStart: settings.auto_start,
      logRetentionDays: settings.log_retention_days,
      failoverMaxAttemptsPerProvider: settings.failover_max_attempts_per_provider,
      failoverMaxProvidersToTry: settings.failover_max_providers_to_try,
      enableKeywordReview: checked,
    });
  };

  const handleTimeoutChange = (value: string) => {
    const num = parseInt(value, 10);
    if (isNaN(num) || num < 1 || num > 3600 || !settings) return;
    settingsMutation.mutate({
      preferredPort: settings.preferred_port,
      autoStart: settings.auto_start,
      logRetentionDays: settings.log_retention_days,
      failoverMaxAttemptsPerProvider: settings.failover_max_attempts_per_provider,
      failoverMaxProvidersToTry: settings.failover_max_providers_to_try,
      keywordReviewTimeoutSeconds: num,
    });
  };

  const handleTimeoutActionChange = (action: "approve" | "reject") => {
    if (!settings) return;
    settingsMutation.mutate({
      preferredPort: settings.preferred_port,
      autoStart: settings.auto_start,
      logRetentionDays: settings.log_retention_days,
      failoverMaxAttemptsPerProvider: settings.failover_max_attempts_per_provider,
      failoverMaxProvidersToTry: settings.failover_max_providers_to_try,
      keywordReviewTimeoutAction: action,
    });
  };

  const keywords = keywordsQuery.data ?? [];

  return (
    <div className="flex flex-col gap-6">
      {/* Settings Section */}
      <div className="rounded-lg border p-4 space-y-4">
        <div className="flex items-center justify-between">
          <div>
            <div className="text-sm font-medium">启用关键词审核</div>
            <div className="text-xs text-muted-foreground">
              开启后，包含敏感词的请求将被拦截等待审批
            </div>
          </div>
          <Switch
            checked={isEnabled}
            onCheckedChange={handleToggleEnabled}
            disabled={settingsMutation.isPending}
          />
        </div>

        {isEnabled && (
          <div className="flex items-center gap-4 text-sm">
            <div className="flex items-center gap-2">
              <span className="text-muted-foreground">审批超时：</span>
              <Input
                type="number"
                min={1}
                max={3600}
                value={timeoutSeconds}
                onChange={(e) => handleTimeoutChange(e.target.value)}
                className="w-20 h-8"
              />
              <span className="text-muted-foreground">秒</span>
            </div>
            <div className="flex items-center gap-2">
              <span className="text-muted-foreground">超时后：</span>
              <Button
                variant={timeoutAction === "reject" ? "primary" : "ghost"}
                size="sm"
                onClick={() => handleTimeoutActionChange("reject")}
              >
                自动拒绝
              </Button>
              <Button
                variant={timeoutAction === "approve" ? "primary" : "ghost"}
                size="sm"
                onClick={() => handleTimeoutActionChange("approve")}
              >
                自动放行
              </Button>
            </div>
          </div>
        )}
      </div>

      {/* Keyword Management */}
      <div className="flex gap-2">
        <Input
          value={newKeyword}
          onChange={(e) => setNewKeyword(e.target.value)}
          placeholder="输入敏感词..."
          onKeyDown={(e) => e.key === "Enter" && handleAdd()}
          className="flex-1"
        />
        <Button onClick={handleAdd} disabled={addMutation.isPending || !newKeyword.trim()}>
          添加
        </Button>
      </div>

      {keywords.length === 0 && (
        <div className="text-sm text-muted-foreground py-8 text-center">
          暂无配置的关键词。添加关键词后，包含这些词的请求将被拦截等待审批。
        </div>
      )}

      <div className="space-y-1">
        {keywords.map((kw) => (
          <div
            key={kw.id}
            className="flex items-center justify-between px-3 py-2 rounded-md hover:bg-muted/50"
          >
            <div className="flex items-center gap-3">
              <Switch
                checked={kw.enabled}
                onCheckedChange={(checked) =>
                  setEnabledMutation.mutate({ id: kw.id, enabled: checked })
                }
              />
              <span
                className={kw.enabled ? "text-foreground" : "text-muted-foreground line-through"}
              >
                {kw.keyword}
              </span>
            </div>
            <Button
              variant="ghost"
              size="icon"
              onClick={() => deleteMutation.mutate(kw.id)}
              className="h-8 w-8 text-muted-foreground hover:text-destructive"
            >
              <Trash2 className="h-4 w-4" />
            </Button>
          </div>
        ))}
      </div>
    </div>
  );
}
