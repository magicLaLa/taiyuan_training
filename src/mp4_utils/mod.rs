use std::io::Cursor;

use mp4::Mp4Reader;
use reqwest::blocking::Client;

/// 通过 mp4 crate 解析视频 duration（秒）
/// 先 HEAD 获取文件大小，再 Range 下载头部数据供 Mp4Reader::read_header 解析
pub fn fetch_mp4_duration(client: &Client, url: &str) -> Result<f64, Box<dyn std::error::Error>> {
    // 1. HEAD 获取文件总大小
    let head_resp = client.head(url).send()?;
    let content_length: u64 = head_resp
        .headers()
        .get("content-length")
        .and_then(|v| v.to_str().ok())
        .and_then(|s| s.parse().ok())
        .ok_or("无法获取 content-length")?;

    println!("  mp4 文件大小: {} 字节", content_length);

    // 2. 逐步增大 Range 直到解析成功（50KB → 200KB → 1MB → 4MB → 16MB）
    for chunk_size in [50 * 1024, 200 * 1024, 1024 * 1024, 4 * 1024 * 1024, 16 * 1024 * 1024] {
        let end = (chunk_size as u64).min(content_length.saturating_sub(1));
        println!("  尝试下载头部 {}KB...", end / 1024);

        let mut partial_resp = client
            .get(url)
            .header("Range", format!("bytes=0-{}", end))
            .send()?;

        let mut buffer = Vec::new();
        partial_resp.copy_to(&mut buffer)?;

        let cursor = Cursor::new(buffer);
        match Mp4Reader::read_header(cursor, content_length) {
            Ok(reader) => {
                let duration = reader.duration().as_secs_f64();
                if duration > 0.0 && duration < 1_000_000.0 {
                    println!("  解析到 duration: {} 秒", duration);
                    return Ok(duration);
                }
            }
            Err(e) => {
                println!("  头部 {}KB 解析失败: {}", end / 1024, e);
            }
        }
    }

    Err("无法从 mp4 中解析 duration".into())
}
