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

export function KeywordsTab() {
  const [newKeyword, setNewKeyword] = useState("");
  const keywordsQuery = useKeywordReviewKeywordsQuery();
  const addMutation = useKeywordReviewKeywordAddMutation();
  const setEnabledMutation = useKeywordReviewKeywordSetEnabledMutation();
  const deleteMutation = useKeywordReviewKeywordDeleteMutation();

  const handleAdd = () => {
    const trimmed = newKeyword.trim();
    if (!trimmed) return;
    addMutation.mutate(trimmed, {
      onSuccess: () => setNewKeyword(""),
    });
  };

  const keywords = keywordsQuery.data ?? [];

  return (
    <div className="flex flex-col gap-4">
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
