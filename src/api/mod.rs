use std::process;

use reqwest::blocking::{multipart, Client};
use std::path::Path;
use serde::Deserialize;

/// 登录响应结构
#[derive(Debug, Deserialize)]
pub struct LoginResponse {
    pub status: Option<i32>,
    pub warn: Option<String>,
    pub data: Option<serde_json::Value>,
}

/// 调用登录 API
pub fn login(
    client: &Client,
    url: &str,
    username: &str,
    password: &str,
) -> Result<LoginResponse, Box<dyn std::error::Error>> {
    let resp = client
        .post(url)
        .header("Content-Type", "application/json")
        .json(&serde_json::json!({ "accountName": username, "password": password }))
        .send()?;

    let status = resp.status();
    let body = resp.text()?;
    if !status.is_success() {
        return Err(format!("HTTP {}: {}", status, body).into());
    }

    let login_resp: LoginResponse = serde_json::from_str(&body)?;
    Ok(login_resp)
}

/// 获取学习进度
pub fn fetch_progress_json(
    client: &Client,
    url: &str,
    user_id: &str,
    start_time: i64,
    end_time: i64,
) -> serde_json::Value {
    let resp = client
        .post(url)
        .header("Content-Type", "application/json")
        .json(&serde_json::json!({
            "state": "",
            "filter": "",
            "pageNo": 0,
            "pageSize": 100,
            "userId": user_id,
            "startTime": start_time,
            "endTime": end_time,
        }))
        .send()
        .expect("fetch_progress 请求失败");

    let body = resp.text().expect("fetch_progress 读取响应失败");
    serde_json::from_str(&body).expect("fetch_progress 解析 JSON 失败")
}

/// putUserParticipateRoom
pub fn put_user_participate_room(
    client: &Client,
    user_id: &str,
    room_id: &str,
) -> Result<String, Box<dyn std::error::Error>> {
    let url = "https://gdyx.bnu.edu.cn/api-web/recordEvaluate/putUserParticipateRoom";
    let resp = client
        .post(url)
        .header("Content-Type", "application/json")
        .json(&serde_json::json!({ "userId": user_id, "roomId": room_id }))
        .send()?;

    let status = resp.status();
    let body = resp.text()?;
    if !status.is_success() {
        return Err(format!("HTTP {}: {}", status, body).into());
    }
    Ok(body)
}

/// getCourseInfo
pub fn get_course_info(
    client: &Client,
    room_id: &str,
    user_id: &str,
) -> serde_json::Value {
    let url = "https://gdyx.bnu.edu.cn/api-web/evaluation/getCourseInfo";
    let resp = client
        .get(url)
        .query(&[("roomId", room_id), ("userId", user_id)])
        .send()
        .expect("getCourseInfo 请求失败");

    let body = resp.text().expect("getCourseInfo 读取响应失败");
    serde_json::from_str(&body).expect("getCourseInfo 解析 JSON 失败")
}

/// joinRoom
pub fn join_room(
    client: &Client,
    user_id: &str,
    room_id: &str,
    start_time: &str,
) -> Result<String, Box<dyn std::error::Error>> {
    let url = "https://gdyx.bnu.edu.cn/api-web/evaluation/joinRoom";
    let resp = client
        .post(url)
        .header("Content-Type", "application/json")
        .json(&serde_json::json!({
            "userId": user_id,
            "roomId": room_id,
            "startTime": start_time,
            "anonymity": false,
        }))
        .send()?;

    let status = resp.status();
    let body = resp.text()?;
    if !status.is_success() {
        return Err(format!("HTTP {}: {}", status, body).into());
    }
    Ok(body)
}

/// getCourseTagAndClockinRecord
pub fn get_course_tag_and_clockin_record(
    client: &Client,
    room_id: &str,
    user_id: &str,
) -> Result<String, Box<dyn std::error::Error>> {
    let url = "https://gdyx.bnu.edu.cn/api-web/evaluation/getCourseTagAndClockinRecord";
    let resp = client
        .get(url)
        .query(&[("roomId", room_id), ("userId", user_id)])
        .send()?;

    let status = resp.status();
    let body = resp.text()?;
    if !status.is_success() {
        return Err(format!("HTTP {}: {}", status, body).into());
    }
    Ok(body)
}

/// getVideoTime
pub fn get_video_time(
    client: &Client,
    room_id: &str,
    user_id: &str,
    video_time: i64,
    all_time: i64,
) -> Result<String, Box<dyn std::error::Error>> {
    let url = "https://gdyx.bnu.edu.cn/api-web/evaluation/getVideoTime";
    let resp = client
        .get(url)
        .query(&[
            ("roomId", room_id),
            ("userId", user_id),
            ("videoTime", &video_time.to_string()),
            ("allTime", &all_time.to_string()),
        ])
        .send()?;

    let status = resp.status();
    let body = resp.text()?;
    if !status.is_success() {
        return Err(format!("HTTP {}: {}", status, body).into());
    }
    Ok(body)
}

/// videoTime（新接口，GET）
pub fn video_time(
    client: &Client,
    room_id: &str,
    user_id: &str,
    video_time: i64,
    all_time: i64,
) -> Result<String, Box<dyn std::error::Error>> {
    let url = "https://gdyx.bnu.edu.cn/api-web/evaluation/videoTime";
    let resp = client
        .get(url)
        .query(&[
            ("roomId", room_id),
            ("userId", user_id),
            ("videoTime", &video_time.to_string()),
            ("allTime", &all_time.to_string()),
        ])
        .send()?;

    let status = resp.status();
    let body = resp.text()?;
    if !status.is_success() {
        return Err(format!("HTTP {}: {}", status, body).into());
    }
    Ok(body)
}

/// 通用检查：响应 JSON 中 status == 10000（兼容数字和字符串）
pub fn check_status_10000(
    result: Result<String, Box<dyn std::error::Error>>,
    label: &str,
) -> serde_json::Value {
    let body = match result {
        Ok(b) => b,
        Err(e) => {
            eprintln!("{} 请求失败: {}", label, e);
            process::exit(1);
        }
    };

    let v: serde_json::Value = match serde_json::from_str(&body) {
        Ok(v) => v,
        Err(_) => {
            println!("{} 响应(非JSON): {}", label, body);
            process::exit(1);
        }
    };

    let status_ok = v.get("status").map_or(false, |s| match s {
        serde_json::Value::String(s) => s == "10000",
        serde_json::Value::Number(n) => n.as_i64() == Some(10000),
        _ => false,
    });

    if !status_ok {
        eprintln!("{} status 不是 10000，响应: {}", label, body);
        process::exit(1);
    }

    println!("{} 成功，status=10000", label);
    v
}

/// 上传公共文件（multipart/form-data，字段名为 file）
pub fn upload_public_file(
    client: &Client,
    url: &str,
    file_path: &Path,
) -> Result<String, Box<dyn std::error::Error>> {
    let file_name = file_path
        .file_name()
        .and_then(|v| v.to_str())
        .ok_or_else(|| format!("无法从路径提取文件名: {}", file_path.display()))?
        .to_string();

    let file_bytes = std::fs::read(file_path)?;
    let part = multipart::Part::bytes(file_bytes)
        .file_name(file_name)
        .mime_str("application/vnd.openxmlformats-officedocument.wordprocessingml.document")?;
    let form = multipart::Form::new().part("file", part);

    let resp = client.post(url).multipart(form).send()?;
    let status = resp.status();
    let body = resp.text()?;
    if !status.is_success() {
        return Err(format!("HTTP {}: {}", status, body).into());
    }

    let v: serde_json::Value = serde_json::from_str(&body)?;
    let upload_status_ok = v.get("status").map_or(false, |s| match s {
        serde_json::Value::String(s) => s == "10000",
        serde_json::Value::Number(n) => n.as_i64() == Some(10000),
        _ => false,
    });

    if !upload_status_ok {
        return Err(format!("上传失败，响应: {}", body).into());
    }

    let file_url = v
        .get("url")
        .and_then(|v| v.as_str())
        .ok_or_else(|| format!("上传响应中缺少 url: {}", body))?;

    Ok(file_url.to_string())
}
