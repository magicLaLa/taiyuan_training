use std::io::Cursor;

use mp4::Mp4Reader;
use reqwest::blocking::Client;

/// 通过 Range 请求头部数据，用 mp4 crate 解析 duration（秒）
pub fn fetch_mp4_duration(client: &Client, url: &str) -> Result<f64, Box<dyn std::error::Error>> {
    // HEAD 获取文件总大小
    let head_resp = client.head(url).send()?;
    let content_length: u64 = head_resp
        .headers()
        .get("content-length")
        .and_then(|v| v.to_str().ok())
        .and_then(|s| s.parse().ok())
        .ok_or("无法获取 content-length")?;

    println!("  mp4 文件大小: {} 字节", content_length);

    // 逐步增大 Range 头部，直到 mp4 crate 解析成功
    for chunk_size in [
        50 * 1024,
        200 * 1024,
        1024 * 1024,
        4 * 1024 * 1024,
        16 * 1024 * 1024,
        64 * 1024 * 1024,
    ] {
        let end = (chunk_size as u64).min(content_length.saturating_sub(1));

        let mut resp = client
            .get(url)
            .header("Range", format!("bytes=0-{}", end))
            .send()?;

        let mut buf = Vec::new();
        resp.copy_to(&mut buf)?;

        let actual_len = buf.len() as u64;
        println!("  尝试头部 {}KB (实际收到 {}KB)...", end / 1024, actual_len / 1024);

        let cursor = Cursor::new(buf);
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

        // 如果实际收到的数据小于请求的 end，说明服务端不支持 Range，直接返回错误
        if actual_len <= end.saturating_sub(1) / 2 {
            return Err("服务端不支持 Range 请求".into());
        }
    }

    Err("无法从 mp4 中解析 duration".into())
}
