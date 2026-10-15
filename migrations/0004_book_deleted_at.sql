-- 软删除（回收站）：deleted_at 非空 = 已移入回收站；
-- NULL = 正常。删除时间与 created_at 同为 RFC3339 文本
ALTER TABLE books ADD COLUMN deleted_at TEXT;
