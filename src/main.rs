use std::path::Path;
use std::process;
use std::time::{SystemTime, UNIX_EPOCH};

use reqwest::blocking::Client;
use reqwest::cookie::Jar;
use serde::Deserialize;
use std::sync::Arc;

/// JSON 配置文件结构
#[derive(Debug, Deserialize)]
struct Config {
    username: String,
    password: String,
}

/// 登录响应结构
#[derive(Debug, Deserialize)]
struct LoginResponse {
    status: Option<i32>,
    warn: Option<String>,
    data: Option<serde_json::Value>,
}

fn main() {
    // 1. 读取配置文件
    let config_path = Path::new("config.json");
    let config: Config = match read_config(config_path) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("读取配置文件失败: {}", e);
            process::exit(1);
        }
    };

    println!("配置读取成功，用户名: {}", config.username);

    // 2. 创建带 Cookie 存储的 HTTP 客户端
    let cookie_jar = Arc::new(Jar::default());
    let client = Client::builder()
        .user_agent("Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36")
        .timeout(std::time::Duration::from_secs(30))
        .cookie_provider(cookie_jar.clone())
        .build()
        .expect("创建 HTTP 客户端失败");

    // 3. 调用登录 API
    let login_url = "https://gdyx.bnu.edu.cn/api-web/manage/login";
    println!("正在登录...");

    let login_data = match login(&client, login_url, &config.username, &config.password) {
        Ok(resp) => {
            let status = resp.status.unwrap_or(0);
            let warn = resp.warn.unwrap_or_default();
            println!("登录成功，status={}", status);
            if !warn.is_empty() {
                println!("warn={}", warn);
            }
            resp.data
        }
        Err(e) => {
            eprintln!("登录失败: {}", e);
            process::exit(1);
        }
    };

    // 4. 从登录 data 中提取 userId
    let user_id = match &login_data {
        Some(data) => data
            .get("userId")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string()),
        None => None,
    };

    let user_id = match user_id {
        Some(id) => {
            println!("提取 userId: {}", id);
            id
        }
        None => {
            eprintln!("无法从登录响应中提取 userId");
            process::exit(1);
        }
    };

    // 5. 计算时间戳（秒级）
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("系统时间异常")
        .as_secs() as i64;

    let year = days_since_epoch_to_year(now / 86400);
    let start_time = year_start_timestamp(year);
    let end_time = now;

    println!(
        "时间范围: {} ~ {} (当前年份: {})",
        start_time, end_time, year
    );

    // 6. 调用学习进度接口
    let progress_url = "https://gdyx.bnu.edu.cn/api-web/stats/user/learning/dashboard/progress";
    println!("\n正在获取学习进度...");

    let progress_data: serde_json::Value =
        match fetch_progress(&client, progress_url, &user_id, start_time, end_time) {
            Ok(resp_body) => {
                match serde_json::from_str(&resp_body) {
                    Ok(v) => {
                        println!(
                            "学习进度响应:\n{}",
                            serde_json::to_string_pretty(&v).unwrap_or(resp_body)
                        );
                        v
                    }
                    Err(_) => {
                        println!("学习进度响应:\n{}", resp_body);
                        process::exit(1);
                    }
                }
            }
            Err(e) => {
                eprintln!("获取学习进度失败: {}", e);
                process::exit(1);
            }
        };

    // 7. 解析 completeRooms，只遍历第一个
    let rooms = progress_data
        .get("completeRooms")
        .and_then(|v| v.as_array());

    match rooms {
        Some(arr) if !arr.is_empty() => {
            // 只取第一个
            if let Some(room) = arr.first() {
                let room_id = room
                    .get("roomId")
                    .and_then(|v| v.as_str())
                    .unwrap_or("");

                println!("\ncompleteRooms 第一个 roomId: {}", room_id);

                if !room_id.is_empty() {
                    match put_user_participate_room(&client, &user_id, room_id) {
                        Ok(resp_body) => {
                            match serde_json::from_str::<serde_json::Value>(&resp_body) {
                                Ok(v) => println!(
                                    "putUserParticipateRoom 响应:\n{}",
                                    serde_json::to_string_pretty(&v).unwrap_or(resp_body)
                                ),
                                Err(_) => println!("putUserParticipateRoom 响应:\n{}", resp_body),
                            }
                        }
                        Err(e) => eprintln!("putUserParticipateRoom 请求失败: {}", e),
                    }
                } else {
                    println!("roomId 为空，跳过");
                }
            }
        }
        Some(_) => println!("\ncompleteRooms 为空数组，无可处理的房间"),
        None => println!("\n响应中未找到 completeRooms 字段"),
    }
}

/// 从 JSON 文件读取配置
fn read_config(path: &Path) -> Result<Config, Box<dyn std::error::Error>> {
    let content = std::fs::read_to_string(path)?;
    let config: Config = serde_json::from_str(&content)?;
    Ok(config)
}

/// 调用登录 API
fn login(
    client: &Client,
    url: &str,
    username: &str,
    password: &str,
) -> Result<LoginResponse, Box<dyn std::error::Error>> {
    let resp = client
        .post(url)
        .header("Content-Type", "application/json")
        .json(&serde_json::json!({
            "accountName": username,
            "password": password
        }))
        .send()?;

    let status = resp.status();
    let body = resp.text()?;

    if !status.is_success() {
        return Err(format!("HTTP {}: {}", status, body).into());
    }

    let login_resp: LoginResponse = serde_json::from_str(&body)?;
    Ok(login_resp)
}

/// 调用学习进度接口
fn fetch_progress(
    client: &Client,
    url: &str,
    user_id: &str,
    start_time: i64,
    end_time: i64,
) -> Result<String, Box<dyn std::error::Error>> {
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
        .send()?;

    let status = resp.status();
    let body = resp.text()?;

    if !status.is_success() {
        return Err(format!("HTTP {}: {}", status, body).into());
    }

    Ok(body)
}

/// 调用 putUserParticipateRoom 接口
fn put_user_participate_room(
    client: &Client,
    user_id: &str,
    room_id: &str,
) -> Result<String, Box<dyn std::error::Error>> {
    let url = "https://gdyx.bnu.edu.cn/api-web/recordEvaluate/putUserParticipateRoom";
    let resp = client
        .post(url)
        .header("Content-Type", "application/json")
        .json(&serde_json::json!({
            "userId": user_id,
            "roomId": room_id,
        }))
        .send()?;

    let status = resp.status();
    let body = resp.text()?;

    if !status.is_success() {
        return Err(format!("HTTP {}: {}", status, body).into());
    }

    Ok(body)
}

/// 将自纪元以来的天数转换为年份（粗略，1970-2100 范围足够）
fn days_since_epoch_to_year(days: i64) -> i64 {
    let mut remaining = days;
    let mut year = 1970;

    loop {
        let days_in_year = if is_leap(year) { 366 } else { 365 };
        if remaining < days_in_year {
            break;
        }
        remaining -= days_in_year;
        year += 1;
        if year > 2100 {
            break;
        }
    }

    year
}

/// 判断闰年
fn is_leap(year: i64) -> bool {
    (year % 4 == 0 && year % 100 != 0) || (year % 400 == 0)
}

/// 获取指定年份起始的 Unix 时间戳（秒）
fn year_start_timestamp(year: i64) -> i64 {
    let mut days = 0;
    for y in 1970..year {
        days += if is_leap(y) { 366 } else { 365 };
    }
    days * 86400
}
