
use std::process;
use std::thread;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use chrono::Local;
use reqwest::blocking::Client;
use reqwest::cookie::Jar;
use std::sync::Arc;

mod config;
mod api;
mod mp4_utils;
mod time_utils;

fn main() {
    // 1. 读取配置文件（优先在 exe 所在目录查找）
    let exe_dir = std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(|d| d.to_path_buf()))
        .unwrap_or_else(|| std::env::current_dir().unwrap());
    let config_path = exe_dir.join("config.json");
    let upload_file_path = exe_dir.join("心得体会.docx");
    let config = match config::read_config(&config_path) {
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
        .timeout(std::time::Duration::from_secs(60))
        .cookie_provider(cookie_jar.clone())
        .build()
        .expect("创建 HTTP 客户端失败");

    // 3. 登录
    let login_url = "https://gdyx.bnu.edu.cn/api-web/manage/login";
    let login_data = api::login(&client, login_url, &config.username, &config.password);
    println!("用户数据加载成功");

    // 4. 提取 userId
    let login_resp = match login_data {
        Ok(resp) => resp,
        Err(e) => {
            eprintln!("登录失败: {}", e);
            process::exit(1);
        }
    };

    let user_id = match &login_resp.data {
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

    // 5. 计算时间戳
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("系统时间异常")
        .as_secs() as i64;

    let year = time_utils::days_since_epoch_to_year(now / 86400);
    let _start_time = time_utils::year_start_timestamp(year);
    let _end_time = now;

    // DateTime::from_timestamp 从时间戳转换并格式化
    let start_str = chrono::DateTime::from_timestamp(_start_time, 0)
        .map(|dt| dt.format("%Y-%m-%d").to_string())
        .unwrap_or_default();
    let end_str = chrono::DateTime::from_timestamp(_end_time, 0)
        .map(|dt| dt.format("%Y-%m-%d %H:%M:%S").to_string())
        .unwrap_or_default();
    println!("查询范围: {} ~ {}", start_str, end_str);

    // 6. 获取学习进度 -> completeRooms
    let progress_url = "https://gdyx.bnu.edu.cn/api-web/stats/user/learning/dashboard/progress";
    println!("\n1\u{fe0f}\u{20e3}  获取学习进度...");
    let progress_data = api::fetch_progress_json(
        &client,
        progress_url,
        &user_id,
        _start_time,
        _end_time,
    );

    let rooms = progress_data
        .get("completeRooms")
        .and_then(|v| v.as_array());

    #[derive(Debug)]
    struct RoomInfo {
        id: String,
        name: String,
        teacher: String,
        rate: String,
    }

    let all_rooms_info: Vec<RoomInfo> = match rooms {
        Some(arr) if !arr.is_empty() => arr
            .iter()
            .filter_map(|r| {
                let id = r.get("roomId")
                    .and_then(|v| v.as_str())
                    .map(|s| s.to_string())?;
                let name = r.get("roomName")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string();
                let teacher = r.get("teacher")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string();
                let mut rate = r.get("complateRate")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string();
                Some(RoomInfo { id, name, teacher, rate })
            })
            .collect(),
        _ => {
            eprintln!("completeRooms 为空或不存在");
            process::exit(1);
        }
    };

    println!("全部房间 ({}) 个:", all_rooms_info.len());
    for (idx, r) in all_rooms_info.iter().enumerate() {
        println!("  {}. roomId={}, roomName={}, teacher={}, complateRate={}", idx + 1, r.id, r.name, r.teacher, r.rate);
    }

    // 过滤掉已完成（complateRate="100.00"）的房间
    let rooms_info: Vec<&RoomInfo> = all_rooms_info
        .iter()
        .filter(|r| r.rate != "100.00")
        .collect();

    if rooms_info.is_empty() {
        println!("\n{}", "━".repeat(50));
        println!("\u{1f389} 所有房间均已学完（complateRate=100.00），无需处理");
        println!("\n按 Enter 键退出...");
        let _ = std::io::stdin().read_line(&mut String::new());
        return;
    }

    println!("\n待处理房间 ({}) 个:", rooms_info.len());
    for (idx, r) in rooms_info.iter().enumerate() {
        println!("  {}. roomId={}, roomName={}, teacher={}, complateRate={}", idx + 1, r.id, r.name, r.teacher, r.rate);
    }



    // 逐个处理房间（串行，确保每个房间完整执行完再处理下一个）
    for (idx, info) in rooms_info.iter().enumerate() {
        println!(
            "\n{} ====== 处理第 {}/{} 个房间 (roomId={}, roomName={}, 讲师={}) ======",
            "━".repeat(44),
            idx + 1,
            rooms_info.len(),
            info.id,
            info.name,
            info.teacher,
        );
        process_single_room(&client, &user_id, &info.id, upload_file_path.as_path());
    }

    println!("\n{}", "━".repeat(50));
    println!("\u{1f389} 全部 {} 个房间处理完成", rooms_info.len());

    println!("\n按 Enter 键退出...");
    let _ = std::io::stdin().read_line(&mut String::new());
}

/// 处理单个 room 的完整流程（步骤 7-18）
fn process_single_room(client: &Client, user_id: &str, room_id: &str, upload_file_path: &std::path::Path) {
    // 7. putUserParticipateRoom
    println!("\n  2\u{fe0f}\u{20e3}  putUserParticipateRoom...");
    api::check_status_10000(
        api::put_user_participate_room(client, user_id, room_id),
        "putUserParticipateRoom",
    );

    // 8. getCourseInfo -> 获取 courseInfo + roomVideoUrl
    println!("\n  3\u{fe0f}\u{20e3}  getCourseInfo...");
    let course_info = api::get_course_info(client, room_id, user_id);
    let status = course_info.get("status").and_then(|v| v.as_str());
    if status != Some("10000") {
        eprintln!("getCourseInfo status 不是 10000: {:?}", status);
        process::exit(1);
    }
    println!("  getCourseInfo 成功，status=10000");

    let course_info_data = course_info
        .get("courseInfo")
        .and_then(|v| v.as_object())
        .cloned()
        .map(|m| serde_json::Value::Object(m))
        .unwrap_or(serde_json::Value::Null);

    let room_video_url = course_info_data
        .get("roomVideoUrl")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());

    let room_video_url = match room_video_url {
        Some(url) if !url.is_empty() => url,
        _ => {
            eprintln!("courseInfo 中没有 roomVideoUrl");
            process::exit(1);
        }
    };

    // 9. joinRoom
    println!("\n  4\u{fe0f}\u{20e3}  joinRoom...");
    let now_str = Local::now().format("%Y-%m-%d %H:%M:%S").to_string();
    api::check_status_10000(
        api::join_room(client, user_id, room_id, &now_str),
        "joinRoom",
    );

    // 10. getCourseTagAndClockinRecord（第一次）
    println!("\n  5\u{fe0f}\u{20e3}  getCourseTagAndClockinRecord（第1次）...");
    api::check_status_10000(
        api::get_course_tag_and_clockin_record(client, room_id, user_id),
        "getCourseTagAndClockinRecord",
    );

    // 11. 获取 mp4 duration
    println!("\n  6\u{fe0f}\u{20e3}  获取视频 duration...");
    let duration_secs = match mp4_utils::fetch_mp4_duration(client, &room_video_url) {
        Ok(d) => {
            println!("  视频 duration: {} 秒", d);
            d
        }
        Err(e) => {
            eprintln!("获取视频 duration 失败: {}", e);
            process::exit(1);
        }
    };

    // 12. 间隔 10 秒
    println!("\n  7\u{fe0f}\u{20e3}  等待 10 秒...");
    thread::sleep(Duration::from_secs(10));

    // 13. getCourseTagAndClockinRecord（第二次）
    println!("\n  8\u{fe0f}\u{20e3}  getCourseTagAndClockinRecord（第2次）...");
    api::check_status_10000(
        api::get_course_tag_and_clockin_record(client, room_id, user_id),
        "getCourseTagAndClockinRecord (2)",
    );

    // 14. getVideoTime（第1次，videoTime=10）
    let all_time = (duration_secs as f64).floor() as i64;
    println!("\n  9\u{fe0f}\u{20e3}  getVideoTime（videoTime=10）...");
    api::check_status_10000(
        api::get_video_time(client, room_id, user_id, 10, all_time),
        "getVideoTime (10s)",
    );

    // 15. 间隔 60 秒
    println!("\n  1\u{fe0f}\u{20e3}0\u{fe0f}\u{20e3}  等待 60 秒...");
    thread::sleep(Duration::from_secs(60));

    // 16. videoTime
    println!("\n  1\u{fe0f}\u{20e3}1\u{fe0f}\u{20e3}  videoTime（all_time-10）...");
    api::check_status_10000(
        api::video_time(client, room_id, user_id, all_time - 10, all_time),
        "videoTime",
    );

    // 17. 等待 10 秒
    println!("\n  1\u{fe0f}\u{20e3}2\u{fe0f}\u{20e3}  等待 10 秒...");
    thread::sleep(Duration::from_secs(10));

    // 18. 轮询：videoTime + getVideoTime 直到 isFinish=1
    println!("\n  1\u{fe0f}\u{20e3}3\u{fe0f}\u{20e3}  轮询检查完成状态...");

    loop {
        let resp = api::get_video_time(client, room_id, user_id, all_time - 5, all_time);

        let body = match resp {
            Ok(b) => b,
            Err(e) => {
                eprintln!("getVideoTime 请求失败: {}", e);
                process::exit(1);
            }
        };

        let v: serde_json::Value = serde_json::from_str(&body).unwrap_or_else(|_| {
            eprintln!("getVideoTime 响应非 JSON: {}", body);
            process::exit(1);
        });

        let status_ok = v.get("status").map_or(false, |s| match s {
            serde_json::Value::String(s) => s == "10000",
            serde_json::Value::Number(n) => n.as_i64() == Some(10000),
            _ => false,
        });

        let is_finish = v.get("isFinish").and_then(|v| v.as_i64()).unwrap_or(0);

        if status_ok && is_finish == 1 {
            println!("  \u{2705} 房间 {} 完成 -- status=10000, isFinish=1", room_id);
            let upload_url = match api::upload_public_file(
                client,
                "https://gdyx.bnu.edu.cn/api-web/upload/uploadPublicFile",
                upload_file_path,
            ) {
                Ok(url) => url,
                Err(e) => {
                    eprintln!("uploadPublicFile 请求失败: {}", e);
                    process::exit(1);
                }
            };
            println!("  📤 上传成功：{}", upload_url);
            break;
        }

        println!("  isFinish={}，未完成，重新 videoTime + 10s 等待...", is_finish);

        api::check_status_10000(
            api::video_time(client, room_id, user_id, all_time - 10, all_time),
            "videoTime (轮询)",
        );

        thread::sleep(Duration::from_secs(10));
    }
}
