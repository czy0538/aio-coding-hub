import { useState } from "react";
import { TabList } from "../ui/shadcn/tab-list";
import { PageHeader } from "../ui/PageHeader";
import { KeywordsTab } from "./keyword-review/KeywordsTab";
import { ReviewLogsTab } from "./keyword-review/ReviewLogsTab";

type Tab = "keywords" | "logs";

const TABS = [
  { key: "keywords" as const, label: "关键词管理" },
  { key: "logs" as const, label: "审核记录" },
];

export function KeywordReviewPage() {
  const [tab, setTab] = useState<Tab>("keywords");

  return (
    <div className="flex flex-col gap-6 h-full overflow-hidden">
      <PageHeader
        title="关键词审核"
        actions={<TabList ariaLabel="关键词审核" items={TABS} value={tab} onChange={setTab} />}
      />
      <div className="flex-1 overflow-auto">
        {tab === "keywords" && <KeywordsTab />}
        {tab === "logs" && <ReviewLogsTab />}
      </div>
    </div>
  );
}
