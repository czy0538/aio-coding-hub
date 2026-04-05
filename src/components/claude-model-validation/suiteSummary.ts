import type { SuiteSummaryRow, SuiteSummary } from "./types";
import { truncateText, firstLine, suitePickEvidenceGrade } from "./helpers";
import { buildSuiteProtocolItems } from "./suiteProtocol";

// ---------------------------------------------------------------------------
// buildSuiteSummary
// ---------------------------------------------------------------------------

export function buildSuiteSummary(
  rows: SuiteSummaryRow[],
  modelNameFallback: string
): SuiteSummary {
  const total = rows.length;
  const done = rows.filter((r) => r.status === "done").length;
  const error = rows.filter((r) => r.status === "error").length;
  const missing = rows.filter((r) => r.status === "missing").length;
  const isRunning = rows.some((r) => r.status === "pending" || r.status === "running");

  const pass = rows.filter((r) => r.status === "done" && r.evaluation.overallPass === true).length;
  const fail = rows.filter((r) => r.status === "done" && r.evaluation.overallPass === false).length;

  const allFinished = !isRunning && done + error + missing === total;
  const overallPass = allFinished
    ? error === 0 && missing === 0 && fail === 0 && pass === done
    : null;

  const modelName = (() => {
    const preferred = rows
      .filter((r) => r.status === "done")
      .map((r) => r.evaluation.derived.modelName)
      .find((m) => m && m !== "\u2014");
    return preferred ?? (modelNameFallback.trim() ? modelNameFallback.trim() : "\u2014");
  })();

  const grades = rows.filter((r) => r.status === "done").map((r) => r.evaluation.grade);
  const grade = suitePickEvidenceGrade(grades);

  const protocol = buildSuiteProtocolItems(rows);

  const issues: SuiteSummary["issues"] = [];
  if (missing > 0) {
    const missingLabels = rows
      .filter((r) => r.status === "missing")
      .map((r) => r.label)
      .slice(0, 4);
    issues.push({
      kind: "error",
      title: `\u5386\u53f2\u7f3a\u5931\u6b65\u9aa4\uff1a${missing}/${total}`,
      detail: missingLabels.length > 0 ? `\u7f3a\u5931\uff1a${missingLabels.join("\uff1b")}` : null,
    });
  }
  for (const r of rows) {
    if (r.status !== "error") continue;
    issues.push({
      kind: "error",
      title: `\u6a21\u677f\u6267\u884c\u5931\u8d25\uff1a${r.label}`,
      detail: r.errorText ? truncateText(firstLine(r.errorText), 160) : null,
    });
  }
  for (const item of protocol) {
    if (!item.required) continue;
    if (item.ok === false) {
      issues.push({
        kind: "error",
        title: `\u534f\u8bae\u4e0d\u6ee1\u8db3\uff1a${item.label}`,
        detail: item.detail,
      });
    } else if (item.ok == null && allFinished && missing === 0 && error === 0) {
      issues.push({
        kind: "warn",
        title: `\u534f\u8bae\u65e0\u6cd5\u5224\u65ad\uff1a${item.label}`,
        detail: item.detail,
      });
    }
  }
  const tamper = protocol.find((p) => p.key === "signature_tamper");
  if (tamper && tamper.ok === false) {
    issues.push({
      kind: "warn",
      title: `\u5f3a\u4fe1\u53f7\u5f02\u5e38\uff1a${tamper.label}`,
      detail: tamper.detail,
    });
  }

  const templateRows: SuiteSummary["templateRows"] = rows.map((r) => ({
    templateKey: r.templateKey,
    label: r.label,
    status: r.status,
    overallPass: r.status === "done" ? r.evaluation.overallPass : null,
    grade: r.status === "done" ? r.evaluation.grade : null,
  }));

  const plainText = (() => {
    const lines: string[] = [];
    const protocolText = isRunning
      ? "\u6267\u884c\u4e2d"
      : overallPass === true
        ? "\u901a\u8fc7"
        : overallPass === false
          ? "\u4e0d\u901a\u8fc7"
          : "\u672a\u77e5";
    const evidenceGrade =
      grade && grade.label !== "\u901a\u8fc7" && grade.label !== "\u672a\u901a\u8fc7"
        ? grade
        : null;

    lines.push(
      "Anthropic Messages API \u9a8c\u8bc1\u603b\u7ed3\uff08/v1/messages\uff0cstream=true\uff09"
    );
    lines.push("");
    lines.push("\u4e00\u3001\u603b\u4f53\u7ed3\u8bba");
    lines.push(`- \u534f\u8bae\u517c\u5bb9\u6027\uff1a${protocolText}`);
    if (evidenceGrade) {
      lines.push(
        `- \u7b2c\u4e00\u65b9\u8bc1\u636e\uff1a${evidenceGrade.level} ${evidenceGrade.label}\uff08${evidenceGrade.title}\uff09`
      );
      lines.push(
        `- \u8bf4\u660e\uff1a\u534f\u8bae\u201c\u901a\u8fc7\u201d\u4ec5\u8868\u793a\u63a5\u53e3\u884c\u4e3a\u7b26\u5408 /v1/messages\uff0c\u4e0d\u7b49\u4ef7\u4e8e\u201c\u7b2c\u4e00\u65b9\u8bc1\u636e A\u201d\u3002`
      );
    } else if (grade) {
      lines.push(`- \u8bc4\u7ea7\uff1a${grade.level} ${grade.label}\uff08${grade.title}\uff09`);
    }
    lines.push(`- \u6a21\u578b\uff1a${modelName}`);
    lines.push(
      `- \u6b65\u9aa4\uff1a\u5b8c\u6210 ${done}/${total}\uff1b\u901a\u8fc7 ${pass}\uff1b\u672a\u901a\u8fc7 ${fail + error + missing}\uff08fail=${fail}; error=${error}; missing=${missing}\uff09`
    );
    lines.push("");
    lines.push("\u4e8c\u3001\u5173\u952e\u68c0\u67e5\u70b9\uff08\u534f\u8bae/\u4fe1\u53f7\uff09");
    for (const p of protocol) {
      const status = p.ok == null ? "\u2014" : p.ok ? "OK" : "FAIL";
      lines.push(
        `- ${p.label}${p.required ? "" : "\uff08\u53c2\u8003\uff09"}\uff1a${status}${p.detail ? `\uff1b${p.detail}` : ""}`
      );
    }
    if (issues.length > 0) {
      lines.push("");
      lines.push("\u672a\u901a\u8fc7/\u98ce\u9669\uff1a");
      for (const it of issues.slice(0, 8)) {
        lines.push(
          `- ${it.kind === "error" ? "ERROR" : "WARN"}\uff1a${it.title}${it.detail ? `\uff1b${it.detail}` : ""}`
        );
      }
    }
    return lines.join("\n");
  })();

  return {
    overallPass,
    isRunning,
    modelName,
    stats: { total, done, pass, fail, error, missing },
    grade,
    templateRows,
    protocol,
    issues,
    plainText,
  };
}
