use tokio::io::{AsyncRead, AsyncReadExt as _};

/// [`Utf8Chunks`] 单次读取的块大小
pub const UTF8_CHUNK_SIZE: usize = 16 * 1024;

/// 按 UTF-8 字符边界分块读取 [`AsyncRead`] 的读取器。
///
/// 不能用 `BufReader::fill_buf` 实现本需求：截断导致 content 接口 500。
/// 因此自管缓冲，把块尾的不完整字符（至多 3 字节）携带到下一块开头拼接。
pub struct Utf8Chunks<R> {
    reader: R,
    /// 复用读缓冲，容量随历史最大块数保留，避免反复分配
    buf: Vec<u8>,
    /// 上一块尾部携带的不完整字符字节
    carry: [u8; 3],
    carry_len: usize,
}

impl<R: AsyncRead + Unpin> Utf8Chunks<R> {
    pub fn new(reader: R) -> Self {
        Self {
            reader,
            buf: Vec::with_capacity(UTF8_CHUNK_SIZE),
            carry: [0; 3],
            carry_len: 0,
        }
    }

    /// 读下一块，保证在字符边界上结束。空串表示 EOF。
    /// 流在多字节字符中间截断、或含非法字节时返回 `InvalidData`。
    pub async fn next_chunk(&mut self) -> std::io::Result<&str> {
        use std::io::{Error, ErrorKind};

        loop {
            // 携带字节置于块首，新数据接在其后
            let carry_len = self.carry_len;
            self.buf.clear();
            self.buf.extend_from_slice(&self.carry[..carry_len]);
            self.buf.resize(carry_len + UTF8_CHUNK_SIZE, 0);
            self.carry_len = 0;

            let n = self.reader.read(&mut self.buf[carry_len..]).await?;
            if n == 0 {
                if carry_len > 0 {
                    return Err(Error::new(
                        ErrorKind::InvalidData,
                        "stream ends within a UTF-8 character",
                    ));
                }
                return Ok("");
            }
            let filled = carry_len + n;

            let valid = match std::str::from_utf8(&self.buf[..filled]) {
                Ok(_) => filled,
                Err(e) if e.error_len().is_some() => {
                    return Err(Error::new(
                        ErrorKind::InvalidData,
                        "invalid UTF-8 in stream",
                    ))
                },
                // 块尾是不完整多字节字符（incomplete 后缀至多 3 字节）：截到其起点
                Err(e) => e.valid_up_to(),
            };

            // valid == 0 说明整块都是某字符的前缀（filled ≤ 3），携带全部继续读
            let tail = filled - valid;
            if tail > 0 {
                self.carry[..tail].copy_from_slice(&self.buf[valid..filled]);
                self.carry_len = tail;
                if valid == 0 {
                    continue;
                }
            }
            return std::str::from_utf8(&self.buf[..valid])
                .map_err(|_| Error::new(ErrorKind::InvalidData, "invalid UTF-8 in stream"));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 每次最多吐 `max` 字节的 reader：`max = 1` 模拟最细粒度分包，
    /// `max = UTF8_CHUNK_SIZE` 模拟文件按块读取
    struct PartialReader<'a> {
        data: &'a [u8],
        pos: usize,
        max: usize,
    }

    impl AsyncRead for PartialReader<'_> {
        fn poll_read(
            mut self: std::pin::Pin<&mut Self>,
            _cx: &mut std::task::Context<'_>,
            buf: &mut tokio::io::ReadBuf<'_>,
        ) -> std::task::Poll<std::io::Result<()>> {
            if self.pos >= self.data.len() {
                return std::task::Poll::Ready(Ok(()));
            }
            let end = (self.pos + self.max).min(self.data.len());
            buf.put_slice(&self.data[self.pos..end]);
            self.pos = end;
            std::task::Poll::Ready(Ok(()))
        }
    }

    async fn collect_chunks<R: AsyncRead + Unpin>(reader: R) -> std::io::Result<String> {
        let mut chunks = Utf8Chunks::new(reader);
        let mut out = String::new();
        loop {
            let chunk = chunks.next_chunk().await?;
            if chunk.is_empty() {
                return Ok(out);
            }
            out.push_str(chunk);
        }
    }

    #[tokio::test]
    async fn utf8_chunks_crossing_block_boundary() {
        // '世' 从第 UTF8_CHUNK_SIZE - 1 字节开始，恰好横跨块边界被截断
        let mut content = String::new();
        for _ in 0..UTF8_CHUNK_SIZE - 1 {
            content.push('a');
        }
        content.push_str("世界🌍bb");

        let out = collect_chunks(PartialReader {
            data: content.as_bytes(),
            pos: 0,
            max: UTF8_CHUNK_SIZE,
        })
        .await
        .unwrap();
        assert_eq!(out, content);
    }

    #[tokio::test]
    async fn utf8_chunks_one_byte_reads() {
        // 逐字节读：不完整字符反复被携带、拼接到下一块
        let content = "a世b界🌍c";
        let out = collect_chunks(PartialReader {
            data: content.as_bytes(),
            pos: 0,
            max: 1,
        })
        .await
        .unwrap();
        assert_eq!(out, content);
    }

    #[tokio::test]
    async fn utf8_chunks_truncated_char_at_eof() {
        // 流在不完整字符处结束：必须报 InvalidData，而不是静默吞掉
        let data = b"ok\xe4\xb8";
        let err = collect_chunks(data.as_slice()).await.unwrap_err();
        assert_eq!(err.kind(), std::io::ErrorKind::InvalidData);
    }

    #[tokio::test]
    async fn utf8_chunks_invalid_bytes() {
        // 含非法字节：拒绝
        let err = collect_chunks(b"ok\xff\xfe".as_slice()).await.unwrap_err();
        assert_eq!(err.kind(), std::io::ErrorKind::InvalidData);
    }
}
