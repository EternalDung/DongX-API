-- 移除 audit_events 审计事件流：对齐参考实现（其无此独立处理）。
-- 安全闸门的 findings 已记录在 request_logs / security_findings 中，
-- 独立的 audit_events 事件流属于冗余数据，故整体删除。
--
-- 历史迁移 001_init.sql 中的 `CREATE TABLE audit_events` 保持不变
-- （迁移铁律：已应用的迁移不可编辑），本迁移以幂等方式 DROP。
DROP TABLE IF EXISTS audit_events;
