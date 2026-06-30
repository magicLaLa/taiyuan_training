use reqwest::StatusCode;
use reqwest::blocking::Client;

const MAX_MOOV_SIZE: u64 = 64 * 1024 * 1024;
const MAX_TOP_LEVEL_BOXES: usize = 128;

/// 通过 Range 定位 moov box，并从 mvhd 中解析 duration（秒）
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

    let mut offset = 0u64;
    for _ in 0..MAX_TOP_LEVEL_BOXES {
        if offset >= content_length {
            break;
        }

        let header_end = (offset + 15).min(content_length.saturating_sub(1));
        let header = fetch_range(client, url, offset, header_end)?;
        let (box_size, box_type, header_size) = parse_box_header(&header).ok_or("MP4 box header 无效")?;

        if box_size != 0 && box_size < header_size as u64 {
            return Err(format!("MP4 box 大小无效: {}", box_size).into());
        }

        let box_name = std::str::from_utf8(&box_type).unwrap_or("????");
        println!("  发现 box {} @ {} ({}KB)", box_name, offset, box_size / 1024);

        if &box_type == b"moov" {
            if box_size > MAX_MOOV_SIZE {
                return Err(format!("moov box 过大: {} 字节", box_size).into());
            }

            let box_end = offset + box_size - 1;
            let moov = fetch_range(client, url, offset, box_end)?;
            let duration = parse_mvhd_duration(&moov[header_size..])
                .ok_or("moov box 中未找到有效 mvhd duration")?;

            println!("  解析到 duration: {} 秒", duration);
            return Ok(duration);
        }

        if box_size == 0 {
            break;
        }

        offset = offset
            .checked_add(box_size)
            .ok_or("MP4 box offset 溢出")?;
    }

    Err("无法在 mp4 顶层 box 中找到 moov".into())
}

fn fetch_range(
    client: &Client,
    url: &str,
    start: u64,
    end: u64,
) -> Result<Vec<u8>, Box<dyn std::error::Error>> {
    let mut resp = client
        .get(url)
        .header("Range", format!("bytes={}-{}", start, end))
        .send()?;

    if resp.status() != StatusCode::PARTIAL_CONTENT {
        return Err(format!("服务端不支持 Range 请求: HTTP {}", resp.status()).into());
    }

    let mut buf = Vec::new();
    resp.copy_to(&mut buf)?;

    let expected_len = end - start + 1;
    if buf.len() < expected_len as usize {
        return Err(format!(
            "Range 返回数据不完整: 期望 {} 字节，实际 {} 字节",
            expected_len,
            buf.len()
        )
        .into());
    }

    Ok(buf)
}

fn parse_box_header(data: &[u8]) -> Option<(u64, [u8; 4], usize)> {
    if data.len() < 8 {
        return None;
    }

    let size32 = u32::from_be_bytes(data[0..4].try_into().ok()?);
    let box_type = data[4..8].try_into().ok()?;

    match size32 {
        0 => Some((0, box_type, 8)),
        1 => {
            if data.len() < 16 {
                return None;
            }
            Some((
                u64::from_be_bytes(data[8..16].try_into().ok()?),
                box_type,
                16,
            ))
        }
        size => Some((size as u64, box_type, 8)),
    }
}

fn parse_mvhd_duration(moov_payload: &[u8]) -> Option<f64> {
    let mut offset = 0usize;

    while offset + 8 <= moov_payload.len() {
        let (box_size, box_type, header_size) = parse_box_header(&moov_payload[offset..])?;
        if box_size == 0 || box_size < header_size as u64 {
            return None;
        }

        let end = offset.checked_add(box_size as usize)?;
        if end > moov_payload.len() {
            return None;
        }

        if &box_type == b"mvhd" {
            return parse_mvhd_payload(&moov_payload[offset + header_size..end]);
        }

        offset = end;
    }

    None
}

fn parse_mvhd_payload(payload: &[u8]) -> Option<f64> {
    let version = *payload.first()?;

    let (timescale_offset, duration_offset, duration_len) = match version {
        0 => (12, 16, 4),
        1 => (20, 24, 8),
        _ => return None,
    };

    let timescale = u32::from_be_bytes(
        payload
            .get(timescale_offset..timescale_offset + 4)?
            .try_into()
            .ok()?,
    );
    if timescale == 0 {
        return None;
    }

    let duration = match duration_len {
        4 => u32::from_be_bytes(
            payload
                .get(duration_offset..duration_offset + 4)?
                .try_into()
                .ok()?,
        ) as u64,
        8 => u64::from_be_bytes(
            payload
                .get(duration_offset..duration_offset + 8)?
                .try_into()
                .ok()?,
        ),
        _ => return None,
    };

    let seconds = duration as f64 / timescale as f64;
    (seconds > 0.0 && seconds < 1_000_000.0).then_some(seconds)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn box_with_payload(name: &[u8; 4], payload: &[u8]) -> Vec<u8> {
        let size = (8 + payload.len()) as u32;
        let mut data = Vec::new();
        data.extend_from_slice(&size.to_be_bytes());
        data.extend_from_slice(name);
        data.extend_from_slice(payload);
        data
    }

    #[test]
    fn parses_duration_from_mvhd_box() {
        let timescale = 1_000u32;
        let duration = 12_345u32;
        let mut mvhd_payload = Vec::new();
        mvhd_payload.extend_from_slice(&[0, 0, 0, 0]);
        mvhd_payload.extend_from_slice(&0u32.to_be_bytes());
        mvhd_payload.extend_from_slice(&0u32.to_be_bytes());
        mvhd_payload.extend_from_slice(&timescale.to_be_bytes());
        mvhd_payload.extend_from_slice(&duration.to_be_bytes());

        let mvhd = box_with_payload(b"mvhd", &mvhd_payload);
        let moov = box_with_payload(b"moov", &mvhd);

        assert_eq!(parse_mvhd_duration(&moov[8..]), Some(12.345));
    }
}
