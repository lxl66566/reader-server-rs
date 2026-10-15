-- 审计日志列表按 created_at 倒序分页（/admin/audit_logs 的 ORDER BY），
-- 日志表增长后全表扫描 + 排序开销显著
CREATE INDEX IF NOT EXISTS idx_audit_logs_created_at ON audit_logs (created_at);
