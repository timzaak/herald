# 决策账本（Decision Logs）

按 feature 一份的决策账本，记录 PRD 引用的 DEC/Q 编号结论。PRD `§7 已确认决策` 中的 `DEC-*` 条目在此对账；被推翻的旧基线也靠这些记录还原取舍理由。

- 文件名与 PRD 一一对应（如 `docs/prd/billing/billing-statistics.md` ↔ `billing-statistics.md`）。
- 状态为 Applied 的决策已落入 PRD 与实现；Deferred/Rejected 条目仅存档。
- 技术设计与技术预研属本地工作层（`.ai/design/`、`.ai/tech-research/`，不随仓库发布），不在本目录。
