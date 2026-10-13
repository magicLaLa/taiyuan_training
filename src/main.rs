
use std::collections::HashSet;
use std::process;
use std::thread;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use chrono::Local;
use reqwest::blocking::Client;
use reqwest::cookie::Jar;
use reqwest::header::{HeaderMap, HeaderValue, AUTHORIZATION};
use std::sync::Arc;

#[derive(Debug, Clone)]
struct RoomInfo {
    id: String,
    name: String,
    teacher: String,
    rate: String,
    is_up_xinde: Option<i64>,
}

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
    let current_dir = std::env::current_dir().unwrap_or_else(|_| exe_dir.clone());
    let exe_config_path = exe_dir.join("config.json");
    let config_path = if exe_config_path.is_file() {
        exe_config_path
    } else {
        current_dir.join("config.json")
    };
    let exe_upload_file_path = exe_dir.join("心得体会.docx");
    let upload_file_path = if exe_upload_file_path.is_file() {
        exe_upload_file_path
    } else {
        current_dir.join("心得体会.docx")
    };
    let config = match config::read_config(&config_path) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("读取配置文件失败: {}", e);
            process::exit(1);
        }
    };

    println!("配置读取成功");

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
            println!("用户 ID 已提取");
            id
        }
        None => {
            eprintln!("无法从登录响应中提取 userId");
            process::exit(1);
        }
    };

    let auth_token = match login_resp.token.as_deref() {
        Some(token) if !token.is_empty() => token,
        _ => {
            eprintln!("登录响应中缺少 token");
            process::exit(1);
        }
    };

    let authorization = match HeaderValue::from_str(&format!("Bearer {}", auth_token)) {
        Ok(value) => value,
        Err(_) => {
            eprintln!("登录响应中的 token 无效");
            process::exit(1);
        }
    };
    let mut default_headers = HeaderMap::new();
    default_headers.insert(AUTHORIZATION, authorization);

    // API 请求需要 token；媒体 URL 使用原客户端，避免向视频服务转发登录凭证。
    let api_client = Client::builder()
        .user_agent("Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36")
        .timeout(std::time::Duration::from_secs(60))
        .cookie_provider(cookie_jar.clone())
        .default_headers(default_headers)
        .build()
        .expect("创建认证 HTTP 客户端失败");

    let args: Vec<String> = std::env::args().skip(1).collect();
    let run_missing_ideas = args.iter().any(|arg| arg == "--upload-missing-ideas");
    let dry_run = args.iter().any(|arg| arg == "--dry-run");
    if dry_run && !run_missing_ideas {
        eprintln!("--dry-run 只能与 --upload-missing-ideas 一起使用");
        process::exit(2);
    }
    if run_missing_ideas {
        if let Err(e) = upload_missing_ideas(
            &api_client,
            &user_id,
            upload_file_path.as_path(),
            dry_run,
        ) {
            eprintln!("补传心得失败: {}", e);
            process::exit(1);
        }
        return;
    }

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

    // 6. 获取学习进度 -> completeRooms + unCompleteRooms
    let progress_url = "https://gdyx.bnu.edu.cn/api-web/stats/user/learning/dashboard/progress";
    println!("\n1\u{fe0f}\u{20e3}  获取学习进度...");
    let progress_data = match api::fetch_progress_json(
        &api_client,
        progress_url,
        &user_id,
        _start_time,
        _end_time,
        0,
        100,
    ) {
        Ok(data) => data,
        Err(e) => {
            eprintln!("获取学习进度失败: {}", e);
            process::exit(1);
        }
    };

    let all_rooms_info = parse_rooms(&progress_data);

    if all_rooms_info.is_empty() {
        eprintln!("completeRooms 和 unCompleteRooms 均为空或不存在");
        process::exit(1);
    }

    println!("全部房间 ({}) 个:", all_rooms_info.len());
    for (idx, r) in all_rooms_info.iter().enumerate() {
        println!("  {}. roomId={}, roomName={}, teacher={}, complateRate={}", idx + 1, r.id, r.name, r.teacher, r.rate);
    }

    // 统一过滤掉已完成（complateRate="100.00"）的房间
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
        process_single_room(
            &api_client,
            &client,
            &user_id,
            &info.id,
            upload_file_path.as_path(),
        );
    }

    println!("\n{}", "━".repeat(50));
    println!("\u{1f389} 全部 {} 个房间处理完成", rooms_info.len());

    println!("\n按 Enter 键退出...");
    let _ = std::io::stdin().read_line(&mut String::new());
}

fn parse_rooms(progress_data: &serde_json::Value) -> Vec<RoomInfo> {
    let mut rooms = Vec::new();
    let mut seen_room_ids = HashSet::new();

    for key in ["completeRooms", "unCompleteRooms"] {
        let Some(room_list) = progress_data.get(key).and_then(|value| value.as_array()) else {
            continue;
        };

        for room in room_list {
            let Some(id) = room.get("roomId").and_then(|value| value.as_str()) else {
                continue;
            };
            if !seen_room_ids.insert(id.to_string()) {
                continue;
            }

            rooms.push(RoomInfo {
                id: id.to_string(),
                name: room
                    .get("roomName")
                    .and_then(|value| value.as_str())
                    .unwrap_or("")
                    .to_string(),
                teacher: room
                    .get("teacher")
                    .and_then(|value| value.as_str())
                    .unwrap_or("")
                    .to_string(),
                rate: room
                    .get("complateRate")
                    .and_then(|value| value.as_str())
                    .unwrap_or("")
                    .to_string(),
                is_up_xinde: room.get("isUpXinde").and_then(parse_i64),
            });
        }
    }

    rooms
}

fn parse_i64(value: &serde_json::Value) -> Option<i64> {
    value
        .as_i64()
        .or_else(|| value.as_str().and_then(|text| text.parse().ok()))
}

fn response_status(response: &serde_json::Value) -> Option<String> {
    response.get("status").and_then(|value| match value {
        serde_json::Value::String(status) => Some(status.clone()),
        serde_json::Value::Number(status) => Some(status.to_string()),
        _ => None,
    })
}

fn parse_count(response: &serde_json::Value, key: &str) -> Result<usize, Box<dyn std::error::Error>> {
    let count = response
        .get(key)
        .and_then(|value| {
            value
                .as_u64()
                .or_else(|| value.as_str().and_then(|text| text.parse().ok()))
        })
        .ok_or_else(|| format!("汇总响应中缺少有效字段 {}", key))?;

    usize::try_from(count).map_err(|_| format!("汇总字段 {} 超出范围", key).into())
}

fn upload_missing_ideas(
    api_client: &Client,
    user_id: &str,
    upload_file_path: &std::path::Path,
    dry_run: bool,
) -> Result<(), Box<dyn std::error::Error>> {
    if !upload_file_path.is_file() {
        return Err(format!("心得文件不存在: {}", upload_file_path.display()).into());
    }
    if std::fs::metadata(upload_file_path)?.len() == 0 {
        return Err(format!("心得文件为空: {}", upload_file_path.display()).into());
    }

    let summary = api::fetch_summary_progress(api_client, user_id)?;
    if response_status(&summary).as_deref() != Some("10000") {
        return Err(format!("查询心得汇总失败，status={:?}", response_status(&summary)).into());
    }

    let total_room_count = parse_count(&summary, "yxRoom")?;
    let completed_course_count = parse_count(&summary, "courseCompleteNum")?;
    let completed_idea_count = parse_count(&summary, "xindeCompleteNum")?;
    if completed_idea_count > completed_course_count {
        return Err("心得完成数大于课程完成数，停止补传以避免误操作".into());
    }

    let expected_missing_count = completed_course_count - completed_idea_count;
    println!(
        "课程总数: {}，已完成课程: {}，已完成心得: {}，待补心得: {}",
        total_room_count, completed_course_count, completed_idea_count, expected_missing_count
    );
    if expected_missing_count == 0 {
        println!("所有已完成课程均已有心得，无需补传");
        return Ok(());
    }

    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)?
        .as_secs() as i64;
    let current_year = time_utils::days_since_epoch_to_year(now / 86400);
    let first_year = (current_year - 10).max(1970);
    let mut matched_rooms = None;

    for start_year in (first_year..=current_year).rev() {
        let progress = api::fetch_progress_json(
            api_client,
            "https://gdyx.bnu.edu.cn/api-web/stats/user/learning/dashboard/progress",
            user_id,
            time_utils::year_start_timestamp(start_year),
            now,
            0,
            200,
        )?;
        if response_status(&progress).as_deref() != Some("10000") {
            return Err(format!(
                "查询 {} 年起的学习进度失败，status={:?}",
                start_year,
                response_status(&progress)
            )
            .into());
        }

        let rooms = parse_rooms(&progress);
        let completed_count = rooms
            .iter()
            .filter(|room| room.rate == "100.00")
            .count();
        if rooms.len() == total_room_count && completed_count == completed_course_count {
            matched_rooms = Some(rooms);
            break;
        }
    }

    let rooms = matched_rooms.ok_or_else(|| {
        format!(
            "未能找到与汇总一致的课程列表（应有 {} 门课程、{} 门已完成）",
            total_room_count, completed_course_count
        )
    })?;

    let candidates: Vec<&RoomInfo> = rooms
        .iter()
        .filter(|room| room.rate == "100.00" && room.is_up_xinde == Some(1))
        .collect();
    if candidates.len() != expected_missing_count {
        return Err(format!(
            "汇总显示缺少 {} 篇，但课程标志识别到 {} 篇；未执行上传",
            expected_missing_count,
            candidates.len()
        )
        .into());
    }

    let mut missing_rooms = Vec::new();
    for room in candidates {
        let idea = api::get_idea(api_client, &room.id, user_id)?;
        match response_status(&idea).as_deref() {
            Some("501") if idea.get("data").is_none_or(serde_json::Value::is_null) => {
                missing_rooms.push(room);
            }
            Some("10000")
                if idea
                    .get("data")
                    .and_then(|data| data.get("fileUrl"))
                    .and_then(|url| url.as_str())
                    .is_some_and(|url| !url.is_empty()) =>
            {
                return Err(format!(
                    "课程“{}”标记为未上传，但心得记录已存在；未执行上传",
                    room.name
                )
                .into());
            }
            status => {
                return Err(format!(
                    "课程“{}”的心得状态无法确认（status={:?}）；未执行上传",
                    room.name, status
                )
                .into());
            }
        }
    }

    if missing_rooms.len() != expected_missing_count {
        return Err(format!(
            "心得接口确认缺少 {} 篇，与汇总缺口 {} 不一致；未执行上传",
            missing_rooms.len(),
            expected_missing_count
        )
        .into());
    }

    println!("待补传课程:");
    for room in &missing_rooms {
        println!("  - {}", room.name);
    }
    if dry_run {
        println!("试运行结束，未上传文件或提交心得记录");
        return Ok(());
    }

    let file_name = upload_file_path
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or_else(|| "无法从心得文件路径提取文件名".to_string())?;
    let upload_url = "https://gdyx.bnu.edu.cn/api-web/upload/uploadPublicFile";

    for room in &missing_rooms {
        let course_info = api::get_course_info(api_client, &room.id, user_id)?;
        if response_status(&course_info).as_deref() != Some("10000") {
            return Err(format!(
                "课程“{}”获取课程信息失败，status={:?}",
                room.name,
                response_status(&course_info)
            )
            .into());
        }
        let course_name = course_info
            .get("courseInfo")
            .and_then(|data| data.get("courseName"))
            .and_then(|value| value.as_str())
            .filter(|name| !name.is_empty())
            .ok_or_else(|| format!("课程“{}”缺少 courseName", room.name))?;

        println!("正在为课程补传心得: {}", room.name);
        let file_url = api::upload_public_file(api_client, upload_url, upload_file_path)?;
        let upload_result = api::upload_idea(
            api_client,
            &room.id,
            user_id,
            course_name,
            file_name,
            &file_url,
        )?;
        let upload_result: serde_json::Value = serde_json::from_str(&upload_result)?;
        if response_status(&upload_result).as_deref() != Some("10000") {
            return Err(format!(
                "课程“{}”心得提交失败，status={:?}",
                room.name,
                response_status(&upload_result)
            )
            .into());
        }

        let verified_idea = api::get_idea(api_client, &room.id, user_id)?;
        let has_file_url = verified_idea
            .get("data")
            .and_then(|data| data.get("fileUrl"))
            .and_then(|url| url.as_str())
            .is_some_and(|url| !url.is_empty());
        if response_status(&verified_idea).as_deref() != Some("10000") || !has_file_url {
            return Err(format!("课程“{}”提交后校验失败", room.name).into());
        }
        println!("已提交并验证: {}", room.name);
    }

    let updated_summary = api::fetch_summary_progress(api_client, user_id)?;
    if response_status(&updated_summary).as_deref() != Some("10000") {
        return Err("补传后重新查询心得汇总失败".into());
    }
    let updated_idea_count = parse_count(&updated_summary, "xindeCompleteNum")?;
    println!(
        "心得完成数更新: {}/{}",
        updated_idea_count, completed_course_count
    );
    if updated_idea_count != completed_course_count {
        return Err(format!(
            "补传后心得完成数为 {}，预期为 {}",
            updated_idea_count, completed_course_count
        )
        .into());
    }

    Ok(())
}

/// 处理单个 room 的完整流程（步骤 7-18）
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_rooms_preserves_xinde_status_and_filters_recovery_candidates() {
        let progress = serde_json::json!({
            "completeRooms": [
                {"roomId": "missing", "roomName": "缺少心得", "complateRate": "100.00", "isUpXinde": 1},
                {"roomId": "uploaded", "roomName": "已上传", "complateRate": "100.00", "isUpXinde": 2},
                {"roomId": "unknown", "roomName": "标记未知", "complateRate": "100.00", "isUpXinde": 0},
                {"roomId": "incomplete", "roomName": "课程未完成", "complateRate": "50.00", "isUpXinde": 1}
            ],
            "unCompleteRooms": [
                {"roomId": "not-finished", "roomName": "未完成房间", "complateRate": "0.00", "isUpXinde": 1}
            ]
        });
        let rooms = parse_rooms(&progress);
        let candidates: Vec<&str> = rooms
            .iter()
            .filter(|room| room.rate == "100.00" && room.is_up_xinde == Some(1))
            .map(|room| room.id.as_str())
            .collect();

        assert_eq!(candidates, vec!["missing"]);
    }
}

fn process_single_room(
    api_client: &Client,
    media_client: &Client,
    user_id: &str,
    room_id: &str,
    upload_file_path: &std::path::Path,
) {
    // 7. putUserParticipateRoom
    println!("\n  2\u{fe0f}\u{20e3}  putUserParticipateRoom...");
    api::check_status_10000(
        api::put_user_participate_room(api_client, user_id, room_id),
        "putUserParticipateRoom",
    );

    // 8. getCourseInfo -> 获取 courseInfo + roomVideoUrl
    println!("\n  3\u{fe0f}\u{20e3}  getCourseInfo...");
    let course_info = match api::get_course_info(api_client, room_id, user_id) {
        Ok(data) => data,
        Err(e) => {
            eprintln!("getCourseInfo 请求失败: {}", e);
            process::exit(1);
        }
    };
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

    let course_name = match course_info_data.get("courseName").and_then(|v| v.as_str()) {
        Some(name) if !name.is_empty() => name.to_string(),
        _ => {
            eprintln!("courseInfo 中没有 courseName");
            process::exit(1);
        }
    };

    let upload_file_name = match upload_file_path.file_name().and_then(|v| v.to_str()) {
        Some(name) if !name.is_empty() => name.to_string(),
        _ => {
            eprintln!("无法从上传文件路径提取文件名");
            process::exit(1);
        }
    };

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
        api::join_room(api_client, user_id, room_id, &now_str),
        "joinRoom",
    );

    // 10. getCourseTagAndClockinRecord（第一次）
    println!("\n  5\u{fe0f}\u{20e3}  getCourseTagAndClockinRecord（第1次）...");
    api::check_status_10000(
        api::get_course_tag_and_clockin_record(api_client, room_id, user_id),
        "getCourseTagAndClockinRecord",
    );

    // 11. 获取 mp4 duration
    println!("\n  6\u{fe0f}\u{20e3}  获取视频 duration...");
    let duration_secs = match mp4_utils::fetch_mp4_duration(media_client, &room_video_url) {
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
        api::get_course_tag_and_clockin_record(api_client, room_id, user_id),
        "getCourseTagAndClockinRecord (2)",
    );

    // 14. getVideoTime（第1次，videoTime=10）
    let all_time = (duration_secs as f64).floor() as i64;
    println!("\n  9\u{fe0f}\u{20e3}  getVideoTime（videoTime=10）...");
    api::check_status_10000(
        api::get_video_time(api_client, room_id, user_id, 10, all_time),
        "getVideoTime (10s)",
    );

    // 15. 间隔 60 秒
    println!("\n  1\u{fe0f}\u{20e3}0\u{fe0f}\u{20e3}  等待 60 秒...");
    thread::sleep(Duration::from_secs(60));

    // 16. videoTime
    println!("\n  1\u{fe0f}\u{20e3}1\u{fe0f}\u{20e3}  videoTime（all_time-10）...");
    api::check_status_10000(
        api::video_time(api_client, room_id, user_id, all_time - 10, all_time),
        "videoTime",
    );

    // 17. 等待 10 秒
    println!("\n  1\u{fe0f}\u{20e3}2\u{fe0f}\u{20e3}  等待 10 秒...");
    thread::sleep(Duration::from_secs(10));

    // 18. 轮询：videoTime + getVideoTime 直到 isFinish=1
    println!("\n  1\u{fe0f}\u{20e3}3\u{fe0f}\u{20e3}  轮询检查完成状态...");

    loop {
        let resp = api::get_video_time(api_client, room_id, user_id, all_time - 5, all_time);

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
            let upload_url = {
                let mut last_err = String::new();
                let mut success = false;
                let mut result = String::new();
                for attempt in 1..=3 {
                    match api::upload_public_file(
                        api_client,
                        "https://gdyx.bnu.edu.cn/api-web/upload/uploadPublicFile",
                        upload_file_path,
                    ) {
                        Ok(url) => {
                            result = url;
                            success = true;
                            break;
                        }
                        Err(e) => {
                            last_err = format!("{}", e);
                            eprintln!("uploadPublicFile 请求失败 (第{}/3 次): {}", attempt, last_err);
                            if attempt < 3 {
                                println!("等待 10 秒后重试...");
                                thread::sleep(Duration::from_secs(10));
                            }
                        }
                    }
                }
                if !success {
                    eprintln!("uploadPublicFile 重试 3 次均失败: {}", last_err);
                    process::exit(1);
                }
                result
            };
            println!("  📤 文件上传成功：{}", upload_url);
            println!("  📝 提交心得记录...");
            api::check_status_10000(
                api::upload_idea(
                    api_client,
                    room_id,
                    user_id,
                    &course_name,
                    &upload_file_name,
                    &upload_url,
                ),
                "uploadIdea",
            );
            break;
        }

        println!("  isFinish={}，未完成，重新 videoTime + 10s 等待...", is_finish);

        api::check_status_10000(
            api::video_time(api_client, room_id, user_id, all_time - 10, all_time),
            "videoTime (轮询)",
        );

        thread::sleep(Duration::from_secs(10));
    }
}
